#![cfg(test)]

use axum::body::{to_bytes, Body};
use http::{header, Request, Response, StatusCode};
use std::sync::Arc;
use tower::{Service, ServiceExt}; // for `call`, `oneshot`, and `ready`
use url::Url;

use super::state::*;
use super::web_result::RawJsonError;
use super::*;

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

#[tokio::test]
async fn api_behaviors() {
    let state = test_state().await;
    // retain a reference to the state for test DB access
    let mut app = eardogger_app(state.clone());

    let user = state.db.test_user("right_user").await.unwrap();

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
        assert!(!resp
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_METHODS));

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
        assert!(!resp
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_METHODS));
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
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let body_bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let err: RawJsonError = serde_json::from_slice(&body_bytes).unwrap();
        assert!(err.error.contains("aren't")); // haha, tiniest fragment of the actual error.
    }
    // 3. Logged in: it lists your dogears.
    {
        let req = Request::builder()
            .uri("/api/v1/list")
            .method("GET")
            .header(header::ACCEPT, "application/json")
            .header(
                header::COOKIE,
                format!("eardogger.sessid={}", &user.session_id),
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
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let body_bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let err: RawJsonError = serde_json::from_slice(&body_bytes).unwrap();
        assert!(err.error.contains("permissions")); // tiniest fragment of the actual error.
    }
}
