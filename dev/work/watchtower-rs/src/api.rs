#![forbid(unsafe_code)]

//! Rust translation of the legacy HTTP API gate.
//!
//! The original Go code had three jobs:
//! - validate the bearer token for registered handlers
//! - remember whether any handler was registered at all
//! - start the HTTP server only when the API is enabled
//!
//! This module keeps those semantics in one place. It also exposes a small
//! request/response surface so the handler wrapping and dispatch logic can be
//! tested without a live socket.

use std::collections::BTreeMap;
use std::error::Error as StdError;
use std::fmt;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

use tracing::{debug, error};

const TOKEN_MISSING_MSG: &str = "api token is empty or has not been set. exiting";
const HTTP_BIND_ADDR: &str = "0.0.0.0:8080";

/// Small snapshot of the API state used for startup decisions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ApiStatus {
    pub has_handlers: bool,
    pub token_is_set: bool,
}

/// Result of evaluating whether the API should start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartDecision {
    /// No handlers were registered, so the API stays disabled.
    Skipped,
    /// The API is enabled and the server may be started elsewhere.
    Start { block: bool },
}

/// Errors produced by the startup gating helper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiError {
    TokenMissing,
    ServerStartFailed(String),
    ServerAcceptFailed(String),
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TokenMissing => f.write_str(TOKEN_MISSING_MSG),
            Self::ServerStartFailed(message) => write!(f, "failed to start HTTP API: {message}"),
            Self::ServerAcceptFailed(message) => {
                write!(f, "HTTP API connection failed: {message}")
            }
        }
    }
}

impl StdError for ApiError {}

/// Legacy request snapshot used by the pure Rust API gate.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: String,
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: String,
}

impl HttpRequest {
    /// Return the `Authorization` header if it exists.
    pub fn authorization(&self) -> Option<&str> {
        self.headers.get("Authorization").map(String::as_str)
    }
}

/// Minimal HTTP response used by the pure Rust API gate.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: String,
}

impl HttpResponse {
    /// Build a plain-text response with the provided status code.
    pub fn plain(status: u16, body: impl Into<String>) -> Self {
        let mut headers = BTreeMap::new();
        headers.insert(
            "Content-Type".to_string(),
            "text/plain; charset=utf-8".to_string(),
        );
        Self {
            status,
            headers,
            body: body.into(),
        }
    }

    /// Return the legacy 401 response.
    pub fn unauthorized() -> Self {
        Self::plain(401, "")
    }

    /// Return the legacy 404 response.
    pub fn not_found() -> Self {
        Self::plain(404, "404 page not found")
    }

    fn status_line(&self) -> &'static str {
        match self.status {
            200 => "200 OK",
            401 => "401 Unauthorized",
            404 => "404 Not Found",
            500 => "500 Internal Server Error",
            _ => "200 OK",
        }
    }
}

/// Trait matching the legacy `http.Handler` shape.
pub trait Handler: Send + Sync {
    fn serve_http(&self, request: &HttpRequest) -> HttpResponse;
}

impl<F> Handler for F
where
    F: Fn(&HttpRequest) -> HttpResponse + Send + Sync,
{
    fn serve_http(&self, request: &HttpRequest) -> HttpResponse {
        self(request)
    }
}

type HandlerFn = Arc<dyn Fn(&HttpRequest) -> HttpResponse + Send + Sync>;

/// In-memory representation of the legacy API guard state.
#[derive(Clone, Default)]
pub struct Api {
    token: String,
    has_handlers: bool,
    routes: BTreeMap<String, HandlerFn>,
}

impl Api {
    /// Create a new API state holder from the configured token.
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            has_handlers: false,
            routes: BTreeMap::new(),
        }
    }

    /// Return the current state snapshot.
    pub fn status(&self) -> ApiStatus {
        ApiStatus {
            has_handlers: self.has_handlers,
            token_is_set: self.token_is_set(),
        }
    }

    /// Mark that at least one API handler was registered.
    pub fn mark_handler_registered(&mut self) {
        self.has_handlers = true;
    }

    /// Return the exact Bearer authorization header expected by the API.
    pub fn expected_authorization(&self) -> String {
        format!("Bearer {}", self.token)
    }

    /// Check whether an incoming `Authorization` header is valid.
    pub fn authorize(&self, authorization: Option<&str>) -> bool {
        authorization.is_some_and(|value| value == self.expected_authorization())
    }

    /// Wrap a handler in the legacy bearer-token guard.
    pub fn require_token<F>(&self, handler: F) -> HandlerFn
    where
        F: Fn(&HttpRequest) -> HttpResponse + Send + Sync + 'static,
    {
        let expected = self.expected_authorization();

        Arc::new(move |request: &HttpRequest| {
            if request.authorization() != Some(expected.as_str()) {
                return HttpResponse::unauthorized();
            }

            debug!("Valid token found.");
            handler(request)
        })
    }

    /// Register a plain handler under the given path.
    pub fn register_func<F>(&mut self, path: impl Into<String>, handler: F)
    where
        F: Fn(&HttpRequest) -> HttpResponse + Send + Sync + 'static,
    {
        self.mark_handler_registered();
        self.routes
            .insert(path.into(), self.require_token(handler));
    }

    /// Register a trait-based handler under the given path.
    pub fn register_handler<H>(&mut self, path: impl Into<String>, handler: H)
    where
        H: Handler + 'static,
    {
        let wrapped = move |request: &HttpRequest| handler.serve_http(request);
        self.register_func(path, wrapped);
    }

    /// Dispatch a request against the registered routes.
    pub fn dispatch(&self, request: &HttpRequest) -> HttpResponse {
        self.routes
            .get(route_path(&request.path))
            .map(|handler| handler(request))
            .unwrap_or_else(HttpResponse::not_found)
    }

    /// Decide whether the API should start.
    ///
    /// The API is skipped when no handler was registered. If handlers are
    /// present, the token must be configured before the caller can continue.
    /// The returned decision only describes whether the server would start and
    /// whether it should block; the actual HTTP server binding stays outside
    /// this module.
    pub fn start_decision(&self, block: bool) -> Result<StartDecision, ApiError> {
        if !self.has_handlers {
            return Ok(StartDecision::Skipped);
        }

        if !self.token_is_set() {
            return Err(ApiError::TokenMissing);
        }

        Ok(StartDecision::Start { block })
    }

    /// Start the HTTP API exactly like the legacy Go entrypoint.
    pub fn start(&self, block: bool) -> Result<(), ApiError> {
        match self.start_decision(block)? {
            StartDecision::Skipped => {
                debug!("Watchtower HTTP API skipped.");
                Ok(())
            }
            StartDecision::Start { block } => {
                if block {
                    self.run_http_server()?;
                } else {
                    let api = self.clone();
                    thread::Builder::new()
                        .name("watchtower-http-api".to_string())
                        .spawn(move || {
                            if let Err(err) = api.run_http_server() {
                                error!("{err}");
                            }
                        })
                        .map_err(|err| ApiError::ServerStartFailed(err.to_string()))?;
                }
                Ok(())
            }
        }
    }

    fn run_http_server(&self) -> Result<(), ApiError> {
        let listener = TcpListener::bind(HTTP_BIND_ADDR)
            .map_err(|err| ApiError::ServerStartFailed(err.to_string()))?;

        for incoming in listener.incoming() {
            match incoming {
                Ok(mut stream) => {
                    if let Err(err) = self.handle_connection(&mut stream) {
                        error!("{err}");
                    }
                }
                Err(err) => return Err(ApiError::ServerAcceptFailed(err.to_string())),
            }
        }

        Ok(())
    }

    fn handle_connection(&self, stream: &mut TcpStream) -> io::Result<()> {
        let request = parse_request(stream)?;
        let response = self.dispatch(&request);
        write_response(stream, &response)
    }

    fn token_is_set(&self) -> bool {
        !self.token.is_empty()
    }
}

fn parse_request(stream: &mut TcpStream) -> io::Result<HttpRequest> {
    let mut reader = BufReader::new(stream.try_clone()?);

    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    if request_line.trim().is_empty() {
        return Ok(HttpRequest::default());
    }

    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();

    let mut headers = BTreeMap::new();
    loop {
        let mut line = String::new();
        let bytes = reader.read_line(&mut line)?;
        if bytes == 0 || line == "\r\n" || line == "\n" {
            break;
        }

        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_string(), value.trim().to_string());
        }
    }

    let body_len = headers
        .get("Content-Length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0_u8; body_len];
    if body_len > 0 {
        reader.read_exact(&mut body)?;
    }

    Ok(HttpRequest {
        method,
        path,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

fn route_path(path: &str) -> &str {
    path.split_once('?').map_or(path, |(route, _)| route)
}

fn write_response(stream: &mut TcpStream, response: &HttpResponse) -> io::Result<()> {
    let body = response.body.as_bytes();
    let mut headers = response.headers.clone();
    headers.insert("Content-Length".to_string(), body.len().to_string());

    write!(stream, "HTTP/1.1 {}\r\n", response.status_line())?;
    for (name, value) in headers {
        write!(stream, "{name}: {value}\r\n")?;
    }
    write!(stream, "\r\n")?;
    stream.write_all(body)?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "123123123";

    fn api() -> Api {
        Api::new(TOKEN)
    }

    fn test_handler(request: &HttpRequest) -> HttpResponse {
        HttpResponse::plain(200, format!("Hello! {}", request.path))
    }

    #[test]
    fn authorize_accepts_matching_bearer_header() {
        let api = Api::new("secret-token");

        assert!(api.authorize(Some("Bearer secret-token")));
    }

    #[test]
    fn authorize_rejects_missing_or_wrong_header() {
        let api = Api::new("secret-token");

        assert!(!api.authorize(None));
        assert!(!api.authorize(Some("Bearer wrong")));
        assert!(!api.authorize(Some("Basic secret-token")));
    }

    #[test]
    fn status_reflects_handler_registration_and_token_presence() {
        let mut api = Api::new("secret-token");

        assert_eq!(
            api.status(),
            ApiStatus {
                has_handlers: false,
                token_is_set: true,
            }
        );

        api.mark_handler_registered();

        assert_eq!(
            api.status(),
            ApiStatus {
                has_handlers: true,
                token_is_set: true,
            }
        );
    }

    #[test]
    fn require_token_rejects_missing_or_wrong_header() {
        let api = api();
        let handler = api.require_token(test_handler);

        let request = HttpRequest::default();
        assert_eq!(handler(&request), HttpResponse::unauthorized());

        let request = HttpRequest {
            headers: BTreeMap::from([("Authorization".to_string(), "Bearer wrong".to_string())]),
            ..HttpRequest::default()
        };
        assert_eq!(handler(&request), HttpResponse::unauthorized());
    }

    #[test]
    fn require_token_runs_the_handler_for_a_valid_header() {
        let api = api();
        let handler = api.require_token(test_handler);
        let request = HttpRequest {
            path: "/hello".to_string(),
            headers: BTreeMap::from([(
                "Authorization".to_string(),
                api.expected_authorization(),
            )]),
            ..HttpRequest::default()
        };

        assert_eq!(handler(&request).status, 200);
        assert_eq!(handler(&request).body, "Hello! /hello");
    }

    #[test]
    fn register_func_and_dispatch_use_the_token_wrapper() {
        let mut api = api();
        api.register_func("/hello", test_handler);

        let request = HttpRequest {
            path: "/hello".to_string(),
            headers: BTreeMap::from([(
                "Authorization".to_string(),
                api.expected_authorization(),
            )]),
            ..HttpRequest::default()
        };

        assert_eq!(api.dispatch(&request), HttpResponse::plain(200, "Hello! /hello"));
    }

    #[test]
    fn dispatch_matches_registered_routes_when_the_request_has_a_query_string() {
        let mut api = api();
        api.register_func("/hello", test_handler);

        let request = HttpRequest {
            path: "/hello?image=nginx&image=redis".to_string(),
            headers: BTreeMap::from([(
                "Authorization".to_string(),
                api.expected_authorization(),
            )]),
            ..HttpRequest::default()
        };

        assert_eq!(
            api.dispatch(&request),
            HttpResponse::plain(200, "Hello! /hello?image=nginx&image=redis")
        );
    }

    struct HandlerObject;

    impl Handler for HandlerObject {
        fn serve_http(&self, request: &HttpRequest) -> HttpResponse {
            HttpResponse::plain(200, format!("Object {}", request.path))
        }
    }

    #[test]
    fn register_handler_accepts_trait_based_handlers() {
        let mut api = api();
        api.register_handler("/object", HandlerObject);

        let request = HttpRequest {
            path: "/object".to_string(),
            headers: BTreeMap::from([(
                "Authorization".to_string(),
                api.expected_authorization(),
            )]),
            ..HttpRequest::default()
        };

        assert_eq!(api.dispatch(&request), HttpResponse::plain(200, "Object /object"));
    }

    #[test]
    fn start_decision_skips_when_no_handlers_exist() {
        let api = Api::new("");

        assert_eq!(api.start_decision(true).unwrap(), StartDecision::Skipped);
        assert_eq!(api.start_decision(false).unwrap(), StartDecision::Skipped);
    }

    #[test]
    fn start_decision_errors_when_handlers_exist_without_token() {
        let mut api = Api::new("");
        api.mark_handler_registered();

        let err = api.start_decision(true).unwrap_err();
        assert_eq!(err, ApiError::TokenMissing);
    }

    #[test]
    fn start_decision_returns_start_when_token_is_present() {
        let mut api = Api::new("abc123");
        api.mark_handler_registered();

        assert_eq!(
            api.start_decision(true).unwrap(),
            StartDecision::Start { block: true }
        );
        assert_eq!(
            api.start_decision(false).unwrap(),
            StartDecision::Start { block: false }
        );
    }

    #[test]
    fn expected_authorization_returns_the_exact_bearer_value() {
        let api = Api::new("abc123");

        assert_eq!(api.expected_authorization(), "Bearer abc123");
    }
}
