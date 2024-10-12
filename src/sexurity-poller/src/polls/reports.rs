use super::PollConfiguration;
extern crate cronjob;
use chrono;
use cronjob::CronJob;
use graphql_client::GraphQLQuery;
use sexurity_api::hackerone::{self as hackerone, HackerOneClient};
use sexurity_api::models::{self as models, ReportData};
use sexurity_api::redis::redis::Commands;
use sexurity_api::redis::{load_set_to_vec, redis, redis::cmd, save_vec_to_set};

pub fn start_poll_event_loop(config: &PollConfiguration) {
    let poll_config = config.clone();
    let mut cron = CronJob::new("report_poll", move |_name: &str| {
        let run = run_poll(&poll_config);
        if run.is_err() {
            error!("error while running reports poll {:#?}", run.err().unwrap());
        }
    });

    // Every 6 minutes
    cron.minutes("*/6");
    cron.seconds("0");
    CronJob::start_job_threaded(cron);
    info!("reports: started poll event loop");
}

pub fn run_poll(config: &PollConfiguration) -> Result<(), Box<dyn std::error::Error>> {
    debug!("report poll event: running poll");
    let mut redis_conn = config.redis_client.get_connection()?;
    let last_run_time: Option<String> = cmd("GET")
        .arg(models::redis_keys::REPORTS_POLL_LAST_RUN_TIME)
        .query(&mut redis_conn)?;

    let last_report_data = get_old_reports_data(&mut redis_conn);
    let mut team_name = None;

    if let Some(team_handle) = &config.team_handle {
        let _team_name = hackerone_get_team_name(team_handle, &config.hackerone)?;
        let _ = team_name.insert(_team_name);
    }

    let report_data = hackerone_get_reports_data(team_name, &config.hackerone);
    if report_data.is_err() {
        error!(
            "reports poll event: error getting reports data: {}",
            report_data.err().unwrap()
        );
        return Ok(());
    }

    let report_data = report_data.unwrap();
    debug!(
        "reports poll event: last_run_time {}",
        last_run_time.clone().unwrap_or("-1".into())
    );
    debug!(
        "reports poll event: last_report_data len: {}, current report_data len: {}",
        last_report_data.clone().unwrap_or_default().len(),
        report_data.len()
    );

    if last_run_time.is_none() || last_report_data.is_none() {
        // first run
        save_vec_to_set(
            models::redis_keys::REPORTS_POLL_LAST_DATA.to_string(),
            report_data,
            false,
            &mut redis_conn,
        )?;
        set_last_run_time_now(&mut redis_conn);
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
        redis_conn.publish::<&str, std::string::String, i32>(
            models::redis_keys::REPORTS_QUEUE_PUBSUB,
            queue_item_encoded,
        )?;
    }

    if last_report_data.is_some() {
        let last_report_data = last_report_data.unwrap();
        if !last_report_data.is_empty() && report_data_cloned.is_empty() {
            return Ok(());
        }
    }

    save_vec_to_set(
        models::redis_keys::REPORTS_POLL_LAST_DATA.to_string(),
        report_data_cloned,
        false,
        &mut redis_conn,
    )?;
    set_last_run_time_now(&mut redis_conn);
    info!("reports: ran poll, {} changes", changed.len());

    Ok(())
}

fn set_last_run_time_now(conn: &mut redis::Connection) {
    let now = chrono::Utc::now();
    let ms = now.timestamp_millis();

    cmd("SET")
        .arg(models::redis_keys::REPORTS_POLL_LAST_RUN_TIME)
        .arg(ms)
        .query::<String>(conn)
        .unwrap();
}

#[rustfmt::skip]
fn hackerone_get_team_name(handle: &str, client: &HackerOneClient) -> Result<String, Box<dyn std::error::Error>> {
    let variables = hackerone::team_name_hacktivity_query::Variables {
        handle: handle.to_string(),
    };

    let query = hackerone::TeamNameHacktivityQuery::build_query(variables);
    let response = client.http.post("https://hackerone.com/graphql").json(&query).send()?;

    let data = response.json::<graphql_client::Response<<hackerone::TeamNameHacktivityQuery as GraphQLQuery>::ResponseData>>()?;
    if let Some(errors) = data.errors {
        if !errors.is_empty() {
            return Err(errors.first().unwrap().message.clone().into());
        }
    }

    let team = data.data.unwrap().team.unwrap();
    Ok(team.name)
}

#[rustfmt::skip]
fn hackerone_get_reports_data(team_name: Option<String>, client: &HackerOneClient) -> Result<Vec<models::ReportData>, Box<dyn std::error::Error>> {
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
    let response = client.http.post("https://hackerone.com/graphql").json(&query).send()?;

    let mut result: Vec<models::ReportData> = vec![];
    let data = response.json::<graphql_client::Response<<hackerone::CompleteHacktivitySearchQuery as GraphQLQuery>::ResponseData>>()?;
    if let Some(errors) = data.errors {
        if !errors.is_empty() {
            return Err(errors.first().unwrap().message.clone().into());
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

fn get_old_reports_data(conn: &mut redis::Connection) -> Option<Vec<models::ReportData>> {
    let last_reports_data = load_set_to_vec(
        String::from(models::redis_keys::REPORTS_POLL_LAST_DATA),
        conn,
    )
    .unwrap_or_default();
    let mut data: Vec<models::ReportData> = vec![];

    for d in last_reports_data {
        let deserialized = serde_json::from_str::<models::ReportData>(&d).unwrap();
        data.push(deserialized);
    }

    Some(data)
}
