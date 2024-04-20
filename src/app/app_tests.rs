#![cfg(test)]

use axum::body::{to_bytes, Body};
use http::{header, Request, Response, StatusCode};
use std::sync::Arc;
use tower::{Service, ServiceExt}; // for `call`, `oneshot`, and `ready`
use url::Url;

use super::state::*;
use super::web_result::RawJsonError;
use super::*;

// Right, here's the ground rules for tests in this file. We're taking as
// axiomatic that DB methods like Dogears::destroy work as advertised, bc
// they're already tested over in db_tests. So we don't bother testing cases
// like wrong user ID. We mostly care about the response formats and status
// codes here.

async fn test_state() -> DogState {
    let db = crate::db::Db::new_test_db().await;
    let own_url = Url::parse("http://eardogger.com").unwrap();
    let assets_dir = "public".to_string();
    let config = DogConfig {
        is_prod: false,
        own_url,
        assets_dir,
    };
    let templates = load_templates().unwrap();
    let inner = DSInner {
        db,
        config,
        templates,
        cookie_key: tower_cookies::Key::generate(),
    };
    Arc::new(inner)
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

fn no_cors(resp: &Response<Body>) -> bool {
    !resp
        .headers()
        .contains_key(header::ACCESS_CONTROL_ALLOW_METHODS)
}

/// Returns true if the response is a 403 due to insufficient token scope.
/// This one consumes the response body, so it needs ownership and async.
async fn api_insufficient_permissions(resp: Response<Body>) -> bool {
    let status = resp.status();
    let body_bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    if let Ok(err) = serde_json::from_slice::<RawJsonError>(&body_bytes) {
        status == StatusCode::FORBIDDEN && err.error.contains("permissions")
    } else {
        false // couldn't deserialize
    }
}

/// Returns true if the response is a 401 (no token or login session provided).
/// This one consumes the response body, so it needs ownership and async.
async fn api_unauthenticated(resp: Response<Body>) -> bool {
    let status = resp.status();
    let body_bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    if let Ok(err) = serde_json::from_slice::<RawJsonError>(&body_bytes) {
        status == StatusCode::UNAUTHORIZED && err.error.contains("aren't")
    } else {
        false // couldn't deserialize
    }
}

fn session_cookie(sessid: &str) -> String {
    format!("eardogger.sessid={}", sessid)
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
        let req = Request::builder()
            .uri("/api/v1/list")
            .method("OPTIONS")
            .header(header::ACCEPT, "application/json")
            .header(header::ORIGIN, "https://example.com")
            .body(Body::empty())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert!(no_cors(&resp));

        // plain GET
        let req = Request::builder()
            .uri("/api/v1/list")
            .method("GET")
            .header(header::ACCEPT, "application/json")
            .header(header::ORIGIN, "https://example.com")
            .header(
                header::COOKIE,
                format!("eardogger.sessid={}", &user.session_id),
            )
            .body(Body::empty())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert!(no_cors(&resp));
    }
    // 2. Logged out: 401.
    {
        let req = Request::builder()
            .uri("/api/v1/list")
            .method("GET")
            .header(header::ACCEPT, "application/json")
            .body(Body::empty())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert!(api_unauthenticated(resp).await);
    }
    // 3. Logged in: it lists your dogears.
    {
        let req = Request::builder()
            .uri("/api/v1/list")
            .method("GET")
            .header(header::ACCEPT, "application/json")
            .header(header::COOKIE, session_cookie(&user.session_id))
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
        let req = Request::builder()
            .uri("/api/v1/list")
            .method("GET")
            .header(header::ACCEPT, "application/json")
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", &user.manage_token),
            )
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
        let req = Request::builder()
            .uri("/api/v1/list")
            .method("GET")
            .header(header::ACCEPT, "application/json")
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", &user.write_token),
            )
            .body(Body::empty())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert!(api_insufficient_permissions(resp).await);
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
        let req = Request::builder()
            .uri(&delete_0)
            .method("OPTIONS")
            .header(header::ACCEPT, "application/json")
            .header(header::ORIGIN, "https://example.com")
            .body(Body::empty())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert!(no_cors(&resp));
    }
    // 2. 401 when logged out
    {
        let req = Request::builder()
            .uri("/api/v1/dogear/20566")
            .method("DELETE")
            .header(header::ACCEPT, "application/json")
            .body(Body::empty())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert!(api_unauthenticated(resp).await);
    }
    // 3. 204 on hit
    {
        let req = Request::builder()
            .uri(&delete_0)
            .method("DELETE")
            .header(header::ACCEPT, "application/json")
            .header(header::COOKIE, session_cookie(&user.session_id))
            .body(Body::empty())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }
    // 4. 404 on whiff
    {
        let req = Request::builder()
            .uri(&delete_0) // Second time using this URL, so it's dead
            .method("DELETE")
            .header(header::ACCEPT, "application/json")
            .header(header::COOKIE, session_cookie(&user.session_id))
            .body(Body::empty())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
    // 5. Tokens: Requires manage scope
    {
        let req = Request::builder()
            .uri(&delete_1)
            .method("DELETE")
            .header(header::ACCEPT, "application/json")
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", &user.write_token),
            )
            .body(Body::empty())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert!(api_insufficient_permissions(resp).await);
    }
    // ...second verse, same as the first, this time it works.
    {
        let req = Request::builder()
            .uri(&delete_1)
            .method("DELETE")
            .header(header::ACCEPT, "application/json")
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", &user.manage_token),
            )
            .body(Body::empty())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }
}
