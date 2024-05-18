use super::app_tests::*;

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

#[tokio::test]
async fn api_update_test() {
    use crate::db::Dogear;

    let state = test_state().await;
    let mut app = eardogger_app(state.clone());

    let user = state.db.test_user("whoever").await.unwrap();
    let uri = "/api/v1/update";

    // reusable test case -- wants a new page number for our example comic.
    // success means: 200 and a Vec<Dogear> with all updated bookmarks.
    let closure_cloneable = app.clone(); // so we can mutably borrow `app` in other test cases.
    let happy_path = |num: u32, auth: Auth| {
        let mut app = closure_cloneable.clone();
        // since it's a format string, the json curlies need doubling.
        let body = format!(r#"{{"current": "http://example.com/comic/{}"}}"#, num);
        let req = new_req("POST", uri)
            .json()
            .auth(auth)
            .header(header::ORIGIN, "http://example.com")
            .body(body.into())
            .unwrap();
        async move {
            let resp = do_req(&mut app, req).await;
            assert_eq!(resp.status(), StatusCode::OK);
            let body = body_bytes(resp).await;
            let updated: Vec<Dogear> =
                serde_json::from_slice(&body).expect("wanted Vec<Dogear> back");
            assert_eq!(updated.len(), 1);
            // updated the current value
            assert_eq!(
                updated[0].current,
                format!("http://example.com/comic/{}", num)
            );
            // hit the expected pre-existing prefix from test data
            assert_eq!(updated[0].prefix, "example.com/comic");
            updated
        }
    };

    // 1: CORS is yes, actually.
    // 1.1: write token works, manage token works, login session works.
    {
        // preflight
        let opt_req = new_req("OPTIONS", uri)
            .json()
            .header(header::ORIGIN, "http://example.com")
            .body(Body::empty())
            .unwrap();
        let opt = do_req(&mut app, opt_req).await;
        // u can post
        assert_eq!(
            opt.headers()
                .get(header::ACCESS_CONTROL_ALLOW_METHODS)
                .unwrap(),
            "POST"
        );
        assert_eq!(
            opt.headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .unwrap(),
            "http://example.com"
        );

        // now some real requests
        happy_path(10, Auth::Token(&user.write_token)).await;
        happy_path(13, Auth::Token(&user.manage_token)).await;
        happy_path(14, Auth::Session(&user.session_id)).await;
        // Well, never mind that a session request prolly wouldn't come with an Origin header...
    }
    // 2. CORS from wrong origin is 404 even if matching bookmark exists.
    {
        let body = r#"{
            "current": "http://example.com/comic/12"
        }"#;
        let req = new_req("POST", uri)
            .json()
            .token(&user.write_token)
            .header(header::ORIGIN, "http://example.horse")
            .body(body.into())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let _ = api_error_body(resp).await.expect("need error body");
    }
    // 3. 401 when not authenticated
    {
        let body = r#"{
            "current": "http://example.com/comic/12"
        }"#;
        let req = new_req("POST", uri)
            .json()
            .header(header::ORIGIN, "http://example.com")
            .body(body.into())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let _ = api_error_body(resp).await.expect("need error body");
    }
    // 4. Busted request: unprocessable
    {
        let body = r#"{
            "whuh???": "http://example.com/comic/12"
        }"#;
        let req = new_req("POST", uri)
            .json()
            .token(&user.write_token)
            .body(body.into())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        // TODO: The error is coming from the Json extractor's Rejection type,
        // which doesn't match the format of ApiError. (It's a line of plain
        // text message.) I can wrap the extractor to customize the Rejection,
        // but maybe that's more trouble than this is worth, since no one else
        // is using this API but me.
        // https://github.com/tokio-rs/axum/blob/main/examples/customize-extractor-error/src/derive_from_request.rs
        // let _ = api_error_body(resp).await.expect("need error body");
    }
}
