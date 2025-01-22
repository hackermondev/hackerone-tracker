use cronjob::CronJob;
use graphql_client::GraphQLQuery;
use security_api::{
    hackerone::{
        self,
        discovery_query::{DiscoveryQueryOpportunitiesSearchNodes, OpportunitiesQuery, SortInput},
        HackerOneClient,
    },
    models,
    redis::save_vec_to_set,
};

use super::PollConfiguration;

pub fn start_poll_event_loop(config: &PollConfiguration) {
    let poll_config = config.clone();
    let mut cron = CronJob::new("program_poll", move |_name: &str| {
        let run = run_poll(&poll_config);
        if run.is_err() {
            error!("error while running program poll {:#?}", run.err().unwrap());
        }
    });

    // Every 24 hours
    cron.hours("*/24");
    cron.seconds("0");
    CronJob::start_job_threaded(cron);
    info!("programs: started poll event loop");
}

pub fn run_poll(config: &PollConfiguration) -> Result<(), Box<dyn std::error::Error>> {
    debug!("program poll event: running poll");
    let mut redis_conn = config.redis_client.get_connection()?;
    let programs = get_programs(&config.hackerone, None)?;

    info!("got {} programs", programs.len());
    trace!("{:#?}", programs);

    save_vec_to_set(
        models::redis_keys::PROGRAMS.to_string(),
        programs,
        false,
        &mut redis_conn,
    )?;

    Ok(())
}

fn get_programs(
    client: &HackerOneClient,
    after: Option<usize>,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let after = after.unwrap_or(0);
    let variables = hackerone::discovery_query::Variables {
        size: Some(100),
        from: Some(after as i64),
        sort: Some(vec![SortInput {
            field: String::from("launched_at"),
            direction: Some(hackerone::discovery_query::OrderDirection::DESC),
        }]),
        query: OpportunitiesQuery {
            ..Default::default()
        },
        ..Default::default()
    };

    let query = hackerone::DiscoveryQuery::build_query(variables);
    let response = client
        .http
        .post("https://hackerone.com/graphql")
        .json(&query)
        .send()?;

    let data = response.json::<graphql_client::Response<<hackerone::DiscoveryQuery as GraphQLQuery>::ResponseData>>()?;
    if let Some(errors) = data.errors {
        if !errors.is_empty() {
            return Err(errors.first().unwrap().message.clone().into());
        }
    }

    let mut program_names = vec![];
    let programs = data
        .data
        .as_ref()
        .unwrap()
        .opportunities_search
        .as_ref()
        .unwrap()
        .nodes
        .as_ref()
        .unwrap();
    for item in programs {
        if item.is_none() {
            continue;
        }

        let program = item.as_ref().unwrap();
        if let DiscoveryQueryOpportunitiesSearchNodes::OpportunityDocument(program) = program {
            program_names.push(program.handle.clone());
        };
    }

    trace!("{:?}", programs);
    if programs.len() == 100 {
        let mut other_programs = get_programs(client, Some(after + programs.len()))?;
        program_names.append(&mut other_programs);
    }

    Ok(program_names)
}
