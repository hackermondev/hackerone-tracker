use graphql_client::GraphQLQuery;
use sexurity_api::hackerone as hackerone;

fn main() {
    let session_token = "";
    let csrf_token = hackerone::get_hackerone_csrf_token(session_token).unwrap();
    println!("{}", csrf_token);

    let client = hackerone::HackerOneClient::new(csrf_token, session_token.to_string());
    // println!("{}", session_token);

    let variables = hackerone::team_year_thank_query::Variables {
        selected_handle: String::from("roblox"),
        year: Some(2023),
    };

    let query = hackerone::TeamYearThankQuery::build_query(variables);
    let response = client.http.post("https://hackerone.com/graphql").json(&query).send().unwrap();

    // println!("{}", response.text().unwrap());
    let data = response.json::<graphql_client::Response<<hackerone::TeamYearThankQuery as GraphQLQuery>::ResponseData>>().unwrap();
    let query_response = data.data.unwrap();
    println!("{:#?}", query_response);
}
