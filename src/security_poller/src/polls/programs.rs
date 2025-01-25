use graphql_client::GraphQLQuery;
use security_api::{
    hackerone::{
        self,
        discovery_query::{DiscoveryQueryOpportunitiesSearchNodes, OpportunitiesQuery, SortInput},
        HackerOneClient,
    },
    models,
    redis::{self, save_vec_to_set},
};

use super::PollConfiguration;

pub async fn run_poll(config: &PollConfiguration) -> Result<(), anyhow::Error> {
    debug!("running poll");

    let mut kv = redis::get_connection().get().await?;
    let programs = get_all_hackerone_programs(&config.hackerone, None).await?;

    info!("got {} programs", programs.len());
    trace!("{:#?}", programs);

    save_vec_to_set(
        models::redis_keys::PROGRAMS,
        programs,
        false,
        &mut kv,
    ).await?;
    Ok(())
}

async fn get_all_hackerone_programs(
    client: &HackerOneClient,
    after: Option<usize>,
) -> Result<Vec<String>, anyhow::Error> {
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
        .send().await?;

    let data = response.json::<graphql_client::Response<<hackerone::DiscoveryQuery as GraphQLQuery>::ResponseData>>().await?;
    if let Some(errors) = data.errors {
        if !errors.is_empty() {
            return Err(anyhow::Error::msg(errors.first().unwrap().message.clone()));
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
        let mut other_programs = Box::pin(get_all_hackerone_programs(client, Some(after + programs.len()))).await?;
        program_names.append(&mut other_programs);
    }

    Ok(program_names)
}
