#![cfg(test)]

mod api;
mod web;

use axum::body::{to_bytes, Body, Bytes};
use http::{header, request::Builder, Request, Response, StatusCode};
use scraper::{Html, Selector};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tower::{Service, ServiceExt}; // for `call`, `oneshot`, and `ready`

use super::state::*;
use super::web_result::RawJsonError;
use super::*;
use crate::config::DogConfig;

// Right, here's the ground rules for tests in this module. We're taking as
// axiomatic that DB methods like Dogears::destroy work as advertised, bc
// they're already type-checked or tested elsewhere. So don't bother w/ cases
// like wrong user ID. We mostly care about the response formats and status
// codes here.
//
// This file has test helpers. The api and web files have tests.

// SHORTCUTS FOR MAKING THINGS

async fn test_state() -> DogState {
    let db = crate::db::Db::new_test_db().await;
    let config = DogConfig::test_config().unwrap();
    let templates = load_templates().unwrap();
    let inner = DSInner {
        db,
        config,
        templates,
        cookie_key: tower_cookies::Key::generate(),
        task_tracker: TaskTracker::new(),
        cancel_token: CancellationToken::new(),
    };
    Arc::new(inner)
}

/// Shortcut for request builder w/ method and URI.
fn new_req(method: impl AsRef<str>, uri: impl AsRef<str>) -> Builder {
    Request::builder().method(method.as_ref()).uri(uri.as_ref())
}

// Since https://github.com/tokio-rs/axum/pull/1751, axum routers can't handle
// type inferrence for the ServiceExt methods because they're no longer generic
// over the body type. So you have to use the uniform function call syntax, which
// makes a minor mess... which I am choosing to corral into this thing.
async fn do_req(app: &mut axum::Router, req: Request<Body>) -> Response<Body> {
    // gotta love a double-unwrap
    ServiceExt::<Request<Body>>::ready(app)
        .await
        .unwrap()
        .call(req)
        .await
        .unwrap()
}

/// One-shot CSS selector construction
fn sel(s: &str) -> Selector {
    Selector::parse(s).unwrap()
}

enum Auth<'a> {
    Token(&'a str),
    Session(&'a str),
}

/// A few little extension methods for request::Builder.
trait TestRequestBuilder {
    /// Adds bearer auth w/ the provided token cleartext.
    fn token(self, token: &str) -> Self;
    /// Adds session auth cookie w/ the provided session ID.
    fn session(self, session: &str) -> Self;
    /// Convenience wrapper for reusable test cases: takes either token or session.
    fn auth(self, auth: Auth) -> Self;
    /// Sets accept + content-type json.
    fn json(self) -> Self;
}

impl TestRequestBuilder for Builder {
    fn token(self, token: &str) -> Self {
        self.header(header::AUTHORIZATION, format!("Bearer {}", token))
    }
    fn session(self, sessid: &str) -> Self {
        self.header(header::COOKIE, format!("eardogger.sessid={}", sessid))
    }
    fn auth(self, auth: Auth) -> Self {
        match auth {
            Auth::Token(t) => self.token(t),
            Auth::Session(s) => self.session(s),
        }
    }
    fn json(self) -> Self {
        self.header(header::ACCEPT, "application/json")
            .header(header::CONTENT_TYPE, "application/json")
    }
}

/// Convenience extension methods for scraper::Html.
trait HasSelector {
    fn has(&self, selector: &str) -> bool;
}

impl HasSelector for Html {
    fn has(&self, selector: &str) -> bool {
        self.select(&sel(selector)).next().is_some()
    }
}

// TRANSFORMING FORMATS

/// Consumes a response to return the body as a Bytes.
async fn body_bytes(resp: Response<Body>) -> Bytes {
    to_bytes(resp.into_body(), usize::MAX).await.unwrap()
}

/// Borrows a Bytes as a &str for quick .contains() checks. Panics on non-utf8.
fn bytes_str(b: &Bytes) -> &str {
    std::str::from_utf8(b.as_ref()).unwrap()
}

/// Borrows a Bytes as an HTML document
fn bytes_doc(b: &Bytes) -> Html {
    Html::parse_document(bytes_str(b))
}

/// Borrows a Bytes as an HTML fragment
fn bytes_frag(b: &Bytes) -> Html {
    Html::parse_fragment(bytes_str(b))
}

/// Consumes a response to return a RawJsonError (or not).
async fn api_error_body(resp: Response<Body>) -> Result<RawJsonError, serde_json::Error> {
    let body = body_bytes(resp).await;
    serde_json::from_slice::<RawJsonError>(&body)
}

// TESTING OUTCOMES

/// Consumes response. Panics unless it's status 200 and contains the login form.
async fn assert_login_page(resp: Response<Body>) {
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_bytes(resp).await;
    let doc = bytes_doc(&body);
    assert!(has_login_form(&doc));
}

/// Consumes response. Panics unless it's status 200 and contains the
/// specified substring in the body.
async fn assert_page_and_contains(resp: Response<Body>, substr: &str) {
    assert_page_and_contains_all(resp, &[substr]).await;
}

/// Consumes response. Panics unless it's status 200 and contains
/// ALL of the specified substrings in the body.
async fn assert_page_and_contains_all(resp: Response<Body>, substrs: &[&str]) {
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_bytes(resp).await;
    let body_str = bytes_str(&body);
    for &s in substrs {
        assert!(body_str.contains(s));
    }
}

/// Panics if the response excludes the Access-Control-Allow-Methods
/// header (which browsers require in order to permit a CORS request).
fn assert_no_cors(resp: &Response<Body>) {
    assert!(!resp
        .headers()
        .contains_key(header::ACCESS_CONTROL_ALLOW_METHODS));
}

/// Panics unless the response is a 403 due to insufficient token scope.
/// This one consumes the response body, so it needs ownership and async.
async fn assert_api_insufficient_permissions(resp: Response<Body>) {
    let status = resp.status();
    let err = api_error_body(resp).await.unwrap();
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(err.error.contains("permissions"));
}

/// Does an API request without providing any auth, and panics unless the response
/// is the expected 401 code and json error object.
async fn assert_api_auth_required(
    app: &mut Router,
    method: &'static str,
    uri: impl AsRef<str>,
    body: Option<Body>,
) {
    let body = body.unwrap_or(Body::empty());
    let req = Request::builder()
        .uri(uri.as_ref())
        .method(method)
        .json()
        .body(body)
        .unwrap();
    let resp = do_req(app, req).await;
    let status = resp.status();
    let err = api_error_body(resp).await.unwrap();
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(err.error.contains("aren't"));
}

/// Reports whether page has account link and logout form.
fn has_logged_in_nav(doc: &Html) -> bool {
    doc.has("nav a[href='/account']") && doc.has("form#logout")
}

/// Reports whether page is displaying the logit form.
fn has_login_form(doc: &Html) -> bool {
    doc.has(r#"form[action="/login"]"#)
}
