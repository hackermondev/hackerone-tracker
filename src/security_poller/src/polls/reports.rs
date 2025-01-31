use super::PollConfiguration;
extern crate cronjob;
use chrono;
use graphql_client::GraphQLQuery;
use security_api::hackerone::{self as hackerone, HackerOneClient};
use security_api::models::{self as models, ReportData};
use security_api::redis::{self, redis::AsyncCommands};

pub async fn run_poll(config: &PollConfiguration) -> Result<(), anyhow::Error> {
    debug!("running poll");
    let mut kv = redis::get_connection().get().await?;
    let last_run_time: Option<String> = kv.get(models::redis_keys::REPORTS_POLL_LAST_RUN_TIME).await?;

    let last_report_data = get_old_reports_data().await?;
    let mut team_name = None;

    if let Some(team_handle) = &config.team_handle {
        let _team_name = hackerone_get_team_name(team_handle, &config.hackerone).await?;
        let _ = team_name.insert(_team_name);
    }

    let report_data = hackerone_get_reports_data(team_name, &config.hackerone).await?;
    if last_run_time.is_none() || last_report_data.is_none() {
        // first run
        redis::save_vec_to_set(
            models::redis_keys::REPORTS_POLL_LAST_DATA,
            report_data,
            false,
            &mut kv,
        ).await?;
        set_last_run_time_now().await?;
        return Ok(());
    }

    let mut changed: Vec<Vec<models::ReportData>> = Vec::new();
    let report_data_cloned = report_data.clone();
    trace!("old data {:#?}", last_report_data.clone().unwrap());
    for report in report_data {
        let report_id: String = report.id.as_ref().unwrap_or(&"".into()).clone();
        let old_data = last_report_data
            .as_ref()
            .unwrap()
            .iter()
            .find(|p| p.id.as_ref().unwrap_or(&"".into()) == &report_id);

        trace!("{:#?}", report);
        if old_data.is_none() {
            // new report
            let empty = models::ReportData::default();
            let diff: Vec<models::ReportData> = vec![empty, report];
            changed.push(diff);
        } else if !old_data.unwrap().disclosed && report.disclosed {
            let diff: Vec<models::ReportData> = vec![old_data.unwrap().clone(), report.clone()];
            changed.push(diff);
        }
    }

    debug!("reports poll event: changed len: {}", changed.len());
    if !changed.is_empty() {
        let mut queue_item = models::ReportsDataQueueItem {
            id: None,
            team_handle: config.team_handle.clone(),
            diff: changed.clone(),
            created_at: chrono::Utc::now().naive_utc(),
        };

        queue_item.create_id();
        let queue_item_encoded = serde_json::to_string(&queue_item).unwrap();
        kv.publish::<&str, std::string::String, i32>(
            models::redis_keys::REPORTS_QUEUE_PUBSUB,
            queue_item_encoded,
        ).await?;
    }

    if last_report_data.is_some() {
        let last_report_data = last_report_data.unwrap();
        if !last_report_data.is_empty() && report_data_cloned.is_empty() {
            return Ok(());
        }
    }

    redis::save_vec_to_set(
        models::redis_keys::REPORTS_POLL_LAST_DATA,
        report_data_cloned,
        false,
        &mut kv,
    ).await?;
    set_last_run_time_now().await?;

    info!("ran poll, {} changes", changed.len());
    Ok(())
}

async fn set_last_run_time_now() -> Result<(), anyhow::Error> {
    let mut kv = redis::get_connection().get().await?;
    let now = chrono::Utc::now();
    let ms = now.timestamp_millis();

    kv.set::<_, _, ()>(models::redis_keys::REPORTS_POLL_LAST_RUN_TIME, ms).await?;
    Ok(())
}

#[rustfmt::skip]
async fn hackerone_get_team_name(handle: &str, client: &HackerOneClient) -> Result<String, anyhow::Error> {
    let variables = hackerone::team_name_hacktivity_query::Variables {
        handle: handle.to_string(),
    };

    let query = hackerone::TeamNameHacktivityQuery::build_query(variables);
    let response = client.http.post("https://hackerone.com/graphql").json(&query).send().await?;

    let data = response.json::<graphql_client::Response<<hackerone::TeamNameHacktivityQuery as GraphQLQuery>::ResponseData>>().await?;
    if let Some(errors) = data.errors {
        if !errors.is_empty() {
            return Err(anyhow::Error::msg(errors.first().unwrap().message.clone()));
        }
    }

    let team = data.data.unwrap().team.unwrap();
    Ok(team.name)
}

#[rustfmt::skip]
async fn hackerone_get_reports_data(team_name: Option<String>, client: &HackerOneClient) -> Result<Vec<models::ReportData>, anyhow::Error> {
    let mut query_string = String::from("disclosed:true");
    if let Some(team_name) = team_name {
        query_string += &format!("&& team:(\"{}\")", team_name);
    }

    let variables = hackerone::complete_hacktivity_search_query::Variables {
        from: Some(0),
        size: Some(10),
        query_string,
        sort: hackerone::complete_hacktivity_search_query::SortInput {
            direction: Some(hackerone::complete_hacktivity_search_query::OrderDirection::DESC),
            field: String::from("latest_disclosable_activity_at"),
        }
    };

    let query = hackerone::CompleteHacktivitySearchQuery::build_query(variables);
    let response = client.http.post("https://hackerone.com/graphql").json(&query).send().await?;

    let mut result: Vec<models::ReportData> = vec![];
    let data = response.json::<graphql_client::Response<<hackerone::CompleteHacktivitySearchQuery as GraphQLQuery>::ResponseData>>().await?;
    if let Some(errors) = data.errors {
        if !errors.is_empty() {
            return Err(anyhow::Error::msg(errors.first().unwrap().message.clone()));
        }
    }
    
    let reports = data.data.unwrap().search.unwrap().nodes.unwrap();
    for item in reports {
        if item.is_none() {
            continue
        }

        let item = item.unwrap();
        let mut report = ReportData::default();    
        trace!("{:#?}", report);

        if let hackerone::complete_hacktivity_search_query::CompleteHacktivitySearchQuerySearchNodes::HacktivityDocument(_hackerone_report) = item {
            let team = _hackerone_report.team.as_ref().unwrap();
            let hackerone_report = _hackerone_report.report;
            let disclosed = _hackerone_report.disclosed.unwrap_or(false);

            if !disclosed || hackerone_report.is_none() {
                continue
            }

            let hackerone_report = hackerone_report.unwrap();
            report.id = Some(hackerone_report.id.clone());
            report.title = hackerone_report.title.clone();
            report.currency = team.currency.clone().unwrap_or(String::from("(unknown currency)"));
            report.awarded_amount = _hackerone_report.total_awarded_amount.unwrap_or(-1) as f64;
            report.disclosed = true;
            report.url = Some(format!("https://hackerone.com/reports/{}", _hackerone_report.id));
            report.collaboration = _hackerone_report.has_collaboration.unwrap_or(false);
            report.summary = if hackerone_report.report_generated_content.is_some() {
                let summary = hackerone_report.report_generated_content.as_ref().unwrap().hacktivity_summary.clone();
                Some(summary.unwrap_or(String::from("This report does not have a summary")))
            } else {
                None
            };

            report.severity = Some(if _hackerone_report.severity_rating.is_none() {
                String::from("unknown")
            } else {
                _hackerone_report.severity_rating.unwrap().to_lowercase()
            });

            if _hackerone_report.reporter.is_some() {
                report.user_name = _hackerone_report.reporter.as_ref().unwrap().username.clone();
                report.user_id = _hackerone_report.reporter.unwrap().id;
            } else {
                report.user_name = "(unknown)".into();
                report.user_id = "1".into();
            }
        };

        result.push(report);
    }

    Ok(result)
}

async fn get_old_reports_data() -> Result<Option<Vec<models::ReportData>>, anyhow::Error> {
    let mut kv = redis::get_connection().get().await?;
    let last_reports_data = redis::load_set_to_vec(
        models::redis_keys::REPORTS_POLL_LAST_DATA,
        &mut kv,
    ).await?;

    let mut data: Vec<models::ReportData> = vec![];
    for d in last_reports_data {
        let deserialized = serde_json::from_str::<models::ReportData>(&d).unwrap();
        data.push(deserialized);
    }

    Ok(Some(data))
}
