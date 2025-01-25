use graphql_client::GraphQLQuery;
use regex::Regex;
use reqwest::{header, Client, ClientBuilder};
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
            .user_agent("HackerOneTracker (+github.com/hackermondev/hackerone-tracker)")
            .default_headers(headers)
            .connect_timeout(Duration::from_secs(5))
            .http2_prior_knowledge()
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

pub async fn fetch_csrf_token(session_token: &str) -> Result<String, anyhow::Error> {
    let client = Client::new();
    let http_response = client
        .get("https://hackerone.com/bugs")
        .header("cookie", format!("__Host-session={};", session_token))
        .send().await?;

    let http_response = http_response.error_for_status()?;
    let http_response = http_response.text().await?;

    let token = extract_csrf_token(&http_response);
    if token.is_none() {
        return Err(anyhow::Error::msg("unable to find CSRF token in page"));
    }

    Ok(token.unwrap())
}

// GraphQL types
type DateTime = String;
type URI = String;

#[derive(GraphQLQuery, Debug)]
#[graphql(
    schema_path = "../../graphql/schema.graphql",
    query_path = "../../graphql/TeamYearThankQuery.graphql",
    request_derives = "Debug",
    response_derives = "Debug"
)]
pub struct TeamYearThankQuery;

#[derive(GraphQLQuery, Debug)]
#[graphql(
    schema_path = "../../graphql/schema.graphql",
    query_path = "../../graphql/CompleteHacktivitySearchQuery.graphql",
    variables_derives = "Default, PartialEq",
    response_derives = "Debug, PartialEq",
    skip_serializing_none
)]
pub struct CompleteHacktivitySearchQuery;

#[derive(GraphQLQuery, Debug)]
#[graphql(
    schema_path = "../../graphql/schema.graphql",
    query_path = "../../graphql/TeamNameHacktivityQuery.graphql",
    variables_derives = "Default, PartialEq",
    response_derives = "Debug, PartialEq",
    skip_serializing_none
)]
pub struct TeamNameHacktivityQuery;

#[derive(GraphQLQuery, Debug)]
#[graphql(
    schema_path = "../../graphql/schema.graphql",
    query_path = "../../graphql/DiscoveryQuery.graphql",
    variables_derives = "Default, PartialEq",
    response_derives = "Debug, PartialEq",
    skip_serializing_none
)]
pub struct DiscoveryQuery;


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_extract_csrf_token() {
        let csrf_token = "hello_world";
        let mock_page = format!(
            r#"
            <meta name="csrf-param" content="authenticity_token" />
            <meta name="csrf-token" content="{csrf_token}" />
        "#
        );

        let extracted_token = extract_csrf_token(&mock_page);
        assert_eq!(extracted_token.unwrap(), csrf_token);
    }
}
