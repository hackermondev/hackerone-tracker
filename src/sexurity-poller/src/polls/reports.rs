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
    let report_data = get_reports_data(&config.team_handle, &config.hackerone);
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
        last_report_data.clone().unwrap_or(vec![]).len(),
        report_data.len()
    );

    if last_run_time.is_none() || last_report_data.is_none() {
        // first run
        save_vec_to_set(
            models::redis_keys::REPORTS_POLL_LAST_DATA.to_string(),
            report_data,
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
            .into_iter()
            .find(|p| p.id.as_ref().unwrap_or(&"".into()) == &report_id);

        trace!("{:#?}", report);
        if old_data.is_none() {
            // new report
            let empty = models::ReportData::default();
            let diff: Vec<models::ReportData> = vec![empty, report];
            changed.push(diff);
        } else {
            if old_data.unwrap().disclosed == false && report.disclosed == true {
                let diff: Vec<models::ReportData> = vec![old_data.unwrap().clone(), report.clone()];
                changed.push(diff);
            }
        }
    }

    debug!("reports poll event: changed len: {}", changed.len());
    if changed.len() > 0 {
        let mut queue_item = models::ReportsDataQueueItem {
            id: None,
            team_handle: config.team_handle.clone(),
            diff: changed.clone(),
            created_at: chrono::Utc::now(),
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
        if last_report_data.len() > 0 && report_data_cloned.len() < 1 {
            return Ok(());
        }
    }

    save_vec_to_set(
        models::redis_keys::REPORTS_POLL_LAST_DATA.to_string(),
        report_data_cloned,
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

#[cfg_attr(rustfmt, rustfmt_skip)]
fn get_reports_data(handle: &str, client: &HackerOneClient) -> Result<Vec<models::ReportData>, Box<dyn std::error::Error>> {
    let variables = hackerone::team_hacktivity_page_query::Variables {
        count: Some(10),
        order_by: Some(hackerone::team_hacktivity_page_query::HacktivityItemOrderInput {
            field: hackerone::team_hacktivity_page_query::HacktivityOrderFieldEnum::popular,
            direction: hackerone::team_hacktivity_page_query::OrderDirection::DESC,
        }),

        secure_order_by: None,
        where_: Some(hackerone::team_hacktivity_page_query::FiltersHacktivityItemFilterInput {
            team: Box::new(Some(hackerone::team_hacktivity_page_query::FiltersTeamFilterInput {
                handle: Some(hackerone::team_hacktivity_page_query::StringPredicateInput { eq: Some(handle.to_string()), ..Default::default() }),
                ..Default::default()
            })),
            ..Default::default()
        }),

        ..Default::default()
    };

    let query = hackerone::TeamHacktivityPageQuery::build_query(variables);
    let response = client.http.post("https://hackerone.com/graphql").json(&query).send()?;

    let mut result: Vec<models::ReportData> = vec![];
    let data = response.json::<graphql_client::Response<<hackerone::TeamHacktivityPageQuery as GraphQLQuery>::ResponseData>>()?;

    let reports = data.data.unwrap().hacktivity_items.unwrap().hacktivity_list.edges.unwrap();
    for report in reports {
        if report.is_none() {
            continue
        }

        let report = report.unwrap();
        let item = report.hacktivity_item.node;
        if item.is_none() {
            continue
        }

        let item = item.unwrap();
        let mut report = ReportData::default();
        debug!("got report: {:#?}", item);
        
        match item {
            hackerone::team_hacktivity_page_query::HacktivityItemNode::Undisclosed(undisclosed) => {
                report.id = Some(undisclosed.hacktivity_item_undisclosed.id.clone());
                report.currency = undisclosed.hacktivity_item_undisclosed.currency.unwrap_or(String::from("(unknown currency)"));
                report.awarded_amount = undisclosed.hacktivity_item_undisclosed.total_awarded_amount.unwrap_or(-1.0);
                report.disclosed = false;
                report.url = Some(format!("https://hackerone.com/reports/{}", undisclosed.hacktivity_item_undisclosed.id));
                report.collaboration = undisclosed.hacktivity_item_undisclosed.is_collaboration.unwrap_or(false);

                if undisclosed.hacktivity_item_undisclosed.reporter.is_some() {
                    report.user_name = undisclosed.hacktivity_item_undisclosed.reporter.as_ref().unwrap().username.clone();
                    report.user_id = undisclosed.hacktivity_item_undisclosed.reporter.unwrap().id;
                } else {
                    report.user_name = "(unknown)".into();
                    report.user_id = "1".into();
                }
            }
            hackerone::team_hacktivity_page_query::HacktivityItemNode::Disclosed(disclosed) => {
                if disclosed.hacktivity_item_disclosed.report.is_none() {
                    continue;
                }
                
                let hackerone_report = disclosed.hacktivity_item_disclosed.report.unwrap();
                report.id = Some(disclosed.hacktivity_item_disclosed.id.clone());
                report.title = hackerone_report.title;
                report.currency = disclosed.hacktivity_item_disclosed.currency.unwrap_or(String::from("(unknown currency)"));
                report.awarded_amount = disclosed.hacktivity_item_disclosed.total_awarded_amount.unwrap_or(-1.0);
                report.disclosed = true;
                report.url = Some(format!("https://hackerone.com/reports/{}", disclosed.hacktivity_item_disclosed.id));
                report.collaboration = disclosed.hacktivity_item_disclosed.is_collaboration.unwrap_or(false);
                report.summary = if hackerone_report.report_generated_content.is_some() {
                    let summary = hackerone_report.report_generated_content.unwrap().hacktivity_summary;
                    Some(summary.unwrap_or(String::from("This report does not have a summary")))
                } else {
                    None
                };
                report.severity = Some(if disclosed.hacktivity_item_disclosed.severity_rating.is_none() {
                    String::from("unknown")
                } else if disclosed.hacktivity_item_disclosed.severity_rating.as_ref().unwrap() == &hackerone::team_hacktivity_page_query::SeverityRatingEnum::critical {
                    String::from("critical")
                } else if disclosed.hacktivity_item_disclosed.severity_rating.as_ref().unwrap() == &hackerone::team_hacktivity_page_query::SeverityRatingEnum::high {
                    String::from("high")
                } else if disclosed.hacktivity_item_disclosed.severity_rating.as_ref().unwrap() == &hackerone::team_hacktivity_page_query::SeverityRatingEnum::low {
                    String::from("low")
                } else if disclosed.hacktivity_item_disclosed.severity_rating.as_ref().unwrap() == &hackerone::team_hacktivity_page_query::SeverityRatingEnum::medium {
                    String::from("medium")
                } else {
                    String::from("none")
                });

                if disclosed.hacktivity_item_disclosed.reporter.is_some() {
                    report.user_name = disclosed.hacktivity_item_disclosed.reporter.as_ref().unwrap().username.clone();
                    report.user_id = disclosed.hacktivity_item_disclosed.reporter.unwrap().id;
                } else {
                    report.user_name = "(unknown)".into();
                    report.user_id = "1".into();
                }
            }
            _ => {
                continue;
            }
        }

        result.push(report);
    }

    Ok(result)
}

fn get_old_reports_data(conn: &mut redis::Connection) -> Option<Vec<models::ReportData>> {
    let last_reports_data = load_set_to_vec(
        String::from(models::redis_keys::REPORTS_POLL_LAST_DATA),
        conn,
    )
    .unwrap_or(vec![]);
    let mut data: Vec<models::ReportData> = vec![];

    for d in last_reports_data {
        let deserialized = serde_json::from_str::<models::ReportData>(&d).unwrap();
        data.push(deserialized);
    }

    Some(data)
}
