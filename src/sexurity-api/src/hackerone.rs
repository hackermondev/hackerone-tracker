use graphql_client::GraphQLQuery;
use regex::Regex;
use reqwest::blocking::{Client, ClientBuilder};
use reqwest::header;
use std::error::Error;
use std::time::Duration;

#[derive(Clone)]
pub struct HackerOneClient {
    pub csrf_token: Option<String>,
    pub session_token: Option<String>,
    pub http: Client,
}

impl HackerOneClient {
    pub fn new(csrf_token: String, session_token: String) -> Self {
        let cookie_header = format!("__Host-session={}", session_token);
        let mut headers = header::HeaderMap::new();
        headers.insert(
            "x-csrf-token",
            header::HeaderValue::from_str(&csrf_token).unwrap(),
        );
        headers.insert(
            "cookie",
            header::HeaderValue::from_str(&cookie_header).unwrap(),
        );

        let client = ClientBuilder::new()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/113.0.0.0 Safari/537.36 Edg/113.0.1774.50")
            .default_headers(headers)
            .connect_timeout(Duration::from_secs(5))
            .build()
            .unwrap();

        HackerOneClient {
            csrf_token: Some(csrf_token.to_string()),
            session_token: Some(session_token.to_string()),
            http: client,
        }
    }
}

fn extract_csrf_token(html: &str) -> Option<String> {
    let re = Regex::new(r#"<meta name="csrf-token" content="([^"]+)" />"#).unwrap();
    if let Some(captures) = re.captures(html) {
        return Some(captures[1].to_string());
    }

    None
}

pub fn get_hackerone_csrf_token(session_token: &str) -> Result<String, Box<dyn Error>> {
    let client = Client::new();
    let http_response = client
        .get("https://hackerone.com/bugs")
        .header("cookie", format!("__Host-session={};", session_token))
        .send()?
        .text()?;

    let token = extract_csrf_token(&http_response);
    if token.is_none() {
        return Err("Could not extract token from page".into());
    }

    Ok(token.unwrap())
}

// GraphQL types

#[derive(GraphQLQuery, Debug)]
#[graphql(
    schema_path = "../../graphql/schema.graphql",
    query_path = "../../graphql/TeamYearThankQuery.graphql",
    response_derives = "Debug"
)]
pub struct TeamYearThankQuery;

// Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_build_team_year_thank_you_query() {
        let variables = team_year_thank_query::Variables {
            selected_handle: String::from("discord"),
            year: Some(2023),
        };

        let query = TeamYearThankQuery::build_query(variables);
        assert_eq!(query.operation_name, "TeamYearThankQuery");
        assert_eq!(query.variables.selected_handle, "discord");
        assert_eq!(query.variables.year, Some(2023));
    }

    #[test]
    fn can_extract_csrf_token() {
        let csrf_token = "hello_world";
        let fake_page = format!(
            r#"
            <meta name="csrf-param" content="authenticity_token" />
            <meta name="csrf-token" content="{csrf_token}" />
        "#
        );

        let extracted_token = extract_csrf_token(&fake_page);
        assert_eq!(extracted_token.unwrap(), csrf_token);
    }
}
