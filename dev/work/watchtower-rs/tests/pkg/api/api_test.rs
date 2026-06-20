#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use watchtower_rs::api::{Api, HttpRequest, HttpResponse};

const TOKEN: &str = "123123123";

#[test]
fn require_token_returns_401_unauthorized_when_token_is_not_provided() {
    let api = Api::new(TOKEN);
    let handler = api.require_token(test_handler);

    let request = HttpRequest::default();

    assert_eq!(handler(&request), HttpResponse::unauthorized());
}

#[test]
fn require_token_returns_401_unauthorized_when_token_is_invalid() {
    let api = Api::new(TOKEN);
    let handler = api.require_token(test_handler);
    let request = HttpRequest {
        headers: BTreeMap::from([(
            "Authorization".to_string(),
            "Bearer 123".to_string(),
        )]),
        ..HttpRequest::default()
    };

    assert_eq!(handler(&request), HttpResponse::unauthorized());
}

#[test]
fn require_token_returns_200_ok_when_token_is_valid() {
    let api = Api::new(TOKEN);
    let handler = api.require_token(test_handler);
    let request = HttpRequest {
        headers: BTreeMap::from([(
            "Authorization".to_string(),
            format!("Bearer {TOKEN}"),
        )]),
        ..HttpRequest::default()
    };

    assert_eq!(handler(&request).status, 200);
}

fn test_handler(_: &HttpRequest) -> HttpResponse {
    HttpResponse::plain(200, "Hello!")
}
