#![cfg(test)]

use axum::body::{to_bytes, Body};
use http::{header, request::Builder, Request, Response, StatusCode};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tower::{Service, ServiceExt}; // for `call`, `oneshot`, and `ready`

use super::state::*;
use super::web_result::RawJsonError;
use super::*;
use crate::config::DogConfig;

// Right, here's the ground rules for tests in this file. We're taking as
// axiomatic that DB methods like Dogears::destroy work as advertised, bc
// they're already type-checked or tested elsewhere. So don't bother w/ cases
// like wrong user ID. We mostly care about the response formats and status
// codes here.

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

// Being lazy.
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

/// Consumes a response to return a RawJsonError (or not).
async fn api_error_body(resp: Response<Body>) -> Result<RawJsonError, serde_json::Error> {
    let body_bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice::<RawJsonError>(&body_bytes)
}

#[tokio::test]
async fn api_list_test() {
    let state = test_state().await;
    // retain a reference to the state for test DB access
    let mut app = eardogger_app(state.clone());

    let user = state.db.test_user("someone").await.unwrap();

    // List!
    // 1. No cors allowed.
    {
        // OPTIONS
        let req = new_req("OPTIONS", "/api/v1/list")
            .json()
            .header(header::ORIGIN, "https://example.com")
            .body(Body::empty())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert_no_cors(&resp);

        // plain GET
        let req = new_req("GET", "/api/v1/list")
            .json()
            .header(header::ORIGIN, "https://example.com")
            .session(&user.session_id)
            .body(Body::empty())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert_no_cors(&resp);
    }
    // 2. Logged out: 401.
    {
        assert_api_auth_required(&mut app, "GET", "/api/v1/list", None).await;
    }
    // 3. Logged in: it lists your dogears.
    {
        let req = new_req("GET", "/api/v1/list")
            .json()
            .session(&user.session_id)
            .body(Body::empty())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let list: ApiDogearsList = serde_json::from_slice(&body_bytes).unwrap();
        // Innate to test data: you start w/ 2 bookmarks.
        assert_eq!(list.meta.pagination.total_count, 2);
        assert_eq!(list.data.len(), 2);
        assert!(list.data[0].current.contains("example.com"));
    }
    // 4. Token auth: it lists your dogears.
    {
        let req = new_req("GET", "/api/v1/list")
            .json()
            .token(&user.manage_token)
            .body(Body::empty())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let list: ApiDogearsList = serde_json::from_slice(&body_bytes).unwrap();
        // Innate to test data: you start w/ 2 bookmarks.
        assert_eq!(list.meta.pagination.total_count, 2);
        assert_eq!(list.data.len(), 2);
        assert!(list.data[0].current.contains("example.com"));
    }
    // 5. Insufficient scope (we need manage or higher): It bails
    {
        let req = new_req("GET", "/api/v1/list")
            .json()
            .token(&user.write_token)
            .body(Body::empty())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert_api_insufficient_permissions(resp).await;
    }
    // 6. Bad page size: legible 400 error
    {
        let req = new_req("GET", "/api/v1/list?page=1&size=50000")
            .json()
            .token(&user.manage_token)
            .body(Body::empty())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let _ = api_error_body(resp).await;
    }
    // 7: Good page size: 👍🏼
    {
        let req = new_req("GET", "/api/v1/list?page=2&size=1")
            .json()
            .session(&user.session_id)
            .body(Body::empty())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let list: ApiDogearsList = serde_json::from_slice(&body_bytes).unwrap();
        // Innate to test data: you start w/ 2 bookmarks.
        assert_eq!(list.meta.pagination.total_count, 2);
        assert_eq!(list.meta.pagination.total_pages, 2);
        assert_eq!(list.meta.pagination.current_page, 2);
        assert_eq!(list.data.len(), 1);
        assert!(list.data[0].current.contains("example.com"));
    }
}

#[tokio::test]
async fn api_delete_test() {
    let state = test_state().await;
    let mut app = eardogger_app(state.clone());

    let user = state.db.test_user("whoever").await.unwrap();
    // Grab an accurate dogear ID to delete
    let user_id = state
        .db
        .users()
        .by_name(&user.name)
        .await
        .unwrap()
        .unwrap()
        .id;
    let (dogears, _) = state.db.dogears().list(user_id, 1, 50).await.unwrap();
    let delete_0 = format!("/api/v1/dogear/{}", dogears[0].id);
    let delete_1 = format!("/api/v1/dogear/{}", dogears[1].id);

    // 1. No cors preflight approval
    {
        let req = new_req("OPTIONS", &delete_0)
            .json()
            .header(header::ORIGIN, "https://example.com")
            .body(Body::empty())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert_no_cors(&resp);
    }
    // 2. 401 when logged out
    {
        assert_api_auth_required(&mut app, "DELETE", "/api/v1/dogear/20566", None).await;
    }
    // 3. 204 on hit
    {
        let req = new_req("DELETE", &delete_0)
            .json()
            .session(&user.session_id)
            .body(Body::empty())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }
    // 4. 404 on whiff
    {
        let req = new_req("DELETE", &delete_0) // Second time using this URL, so it's dead
            .json()
            .session(&user.session_id)
            .body(Body::empty())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
    // 5. Tokens: Requires manage scope
    {
        let req = new_req("DELETE", &delete_1)
            .method("DELETE")
            .json()
            .token(&user.write_token)
            .body(Body::empty())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert_api_insufficient_permissions(resp).await;
    }
    // ...second verse, same as the first, this time it works.
    {
        let req = new_req("DELETE", &delete_1)
            .json()
            .token(&user.manage_token)
            .body(Body::empty())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }
}

#[tokio::test]
async fn api_create_test() {
    use crate::db::Dogear;

    let state = test_state().await;
    let mut app = eardogger_app(state.clone());

    let user = state.db.test_user("whoever").await.unwrap();
    let uri = "/api/v1/create";

    // 1. No cors preflight approval.
    {
        let body = r#"{
            "prefix": "example.com/cors",
            "current": "http://example.com/cors/0"
        }"#;
        let req = new_req("OPTIONS", uri)
            .json()
            .header(header::ORIGIN, "https://example.com")
            .body(body.into())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert_no_cors(&resp);
    }
    // 2. 401 when not authenticated
    {
        let body = r#"{
            "prefix": "example.com/noone",
            "current": "http://example.com/noone/0"
        }"#;
        assert_api_auth_required(&mut app, "POST", uri, Some(body.into())).await;
    }
    // 3. Happy path: 201 and a dogear
    // (changed from ed.v1, which returned a 1-item array)
    {
        // reusable test case; returns a dogear for further inspection. if u even care.
        let happy_path = |auth: Auth, body: &'static str| {
            let req = new_req("POST", uri)
                .json()
                .auth(auth)
                .body(body.into())
                .unwrap();
            // async closures are unstable... and also I can't retain a &mut to that app
            // after I've returned a future. So, clone.
            let mut app = app.clone();
            async move {
                let resp = do_req(&mut app, req).await;
                assert_eq!(resp.status(), StatusCode::CREATED);
                let body_bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
                // Got back a dogear
                let d: Dogear =
                    serde_json::from_slice(&body_bytes).expect("couldn't deserialize Dogear");
                // Didn't get back an error object
                let e = serde_json::from_slice::<RawJsonError>(&body_bytes);
                assert!(e.is_err());
                d
            }
        };
        // 3.1: logged in
        {
            let body = r#"{
                "prefix": "example.com/login",
                "current": "http://example.com/login/1"
            }"#;
            let d = happy_path(Auth::Session(&user.session_id), body).await;
            assert_eq!(d.display_name, None);
        }
        // 3.2: write token is ok
        {
            let body = r#"{
                "prefix": "example.com/write",
                "current": "http://example.com/write/5",
                "display_name": "write token"
            }"#;
            let d = happy_path(Auth::Token(&user.write_token), body).await;
            assert_eq!(d.display_name.as_deref(), Some("write token"));
        }
        // 3.3: manage token is ok
        {
            let body = r#"{
                "prefix": "example.com/manage",
                "current": "http://example.com/manage/91",
                "display_name": "manage token"
            }"#;
            let _ = happy_path(Auth::Token(&user.manage_token), body).await;
        }
    }
    // 4: Legible 409 conflict err on duplicate create
    {
        let body = r#"{
            "prefix": "example.com/comic",
            "current": "http://example.com/comic/99"
        }"#;
        let req = new_req("POST", uri)
            .json()
            .token(&user.write_token)
            .body(body.into())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let _ = api_error_body(resp).await;
    }
}
