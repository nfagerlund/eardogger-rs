use super::app_tests::*;

// /public and /status
#[tokio::test]
async fn app_basics_noauth_test() {
    let state = test_state().await;
    let mut app = eardogger_app(state.clone());

    // Static file serving is hooked up right
    {
        let req = new_req("GET", "/public/style.css")
            .body(Body::empty())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_page_and_contains(resp, "--color-background").await;
    }

    // Status is hooked up right
    {
        let req = new_req("GET", "/status").body(Body::empty()).unwrap();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }
}

#[tokio::test]
async fn token_isnt_logged_in() {
    let state = test_state().await;
    let mut app = eardogger_app(state.clone());
    let user = state.db.test_user("whoever").await.unwrap();

    // AuthSession extractor is properly hooked up: Providing a token is
    // the same as not being logged in at all, for routes that take an
    // AuthSession rather than an AuthAny. This is the only time I'll
    // test this, other routes can just trust the type assurances.
    {
        let req = new_req("GET", "/")
            .token(&user.manage_token)
            .body(Body::empty())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert_login_page(resp).await;
    }
}

#[tokio::test]
async fn index_test() {
    let state = test_state().await;
    let mut app = eardogger_app(state.clone());
    let user = state.db.test_user("whoever").await.unwrap();

    // No login: serves login page
    {
        let req = new_req("GET", "/").body(Body::empty()).unwrap();
        let resp = do_req(&mut app, req).await;
        assert_login_page(resp).await;
    }
    // Yes login: serves the dogears list.
    {
        let req = new_req("GET", "/")
            .session(&user.session_id)
            .body(Body::empty())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_bytes(resp).await;
        let doc = bytes_doc(&body);
        // Includes main layout content for a logged-in user:
        assert!(has_logged_in_nav(&doc));
        // includes "manual mode" form, for now
        assert!(doc.has("form#update-dogear"));
        // Includes dogears list with all dogears (assumption: test user has 2)
        assert_eq!(doc.select(&sel("#dogears li")).count(), 2);
    }
    // pagination.
    // Assumption: test user has two dogears.
    {
        let req = new_req("GET", "/?size=1")
            .session(&user.session_id)
            .body(Body::empty())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_bytes(resp).await;
        let doc = bytes_doc(&body);
        // dogears list present, has #size items
        assert_eq!(doc.select(&sel("#dogears li")).count(), 1);
        // pagination controls present: next link, but no prev link
        assert!(!doc.has(".pagination-link.pagination-previous"));
        let next = doc
            .select(&sel(".pagination-link.pagination-next"))
            .next()
            .expect("must be present");
        // next link goes to pg 2 w/ specified size
        assert_eq!(next.attr("href").unwrap(), "/?page=2&size=1");
        // has correct fragment URL for in-place swap
        assert_eq!(
            next.attr("data-fragment-url").unwrap(),
            "/fragments/dogears?page=2&size=1"
        );
    }
    // pagination part 2
    {
        let req = new_req("GET", "/?page=2&size=1")
            .session(&user.session_id)
            .body(Body::empty())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        let body = body_bytes(resp).await;
        let doc = bytes_doc(&body);
        // dogears list present, has #size items
        assert_eq!(doc.select(&sel("#dogears li")).count(), 1);
        // pagination controls present: prev link, but no next link
        assert!(!doc.has(".pagination-link.pagination-next"));
        let prev = doc
            .select(&sel(".pagination-link.pagination-previous"))
            .next()
            .expect("must be present");
        // prev link goes to pg 1 w/ specified size
        assert_eq!(prev.attr("href").unwrap(), "/?page=1&size=1");
        // has correct fragment URL for in-place swap
        assert_eq!(
            prev.attr("data-fragment-url").unwrap(),
            "/fragments/dogears?page=1&size=1"
        );
    }
}

#[tokio::test]
async fn fragment_dogears_test() {
    let state = test_state().await;
    let mut app = eardogger_app(state.clone());
    let user = state.db.test_user("whoever").await.unwrap();

    // Logged out: 401
    {
        let req = new_req("GET", "/fragments/dogears")
            .body(Body::empty())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
    // Logged in: like the main part of index page.
    {
        let req = new_req("GET", "/fragments/dogears")
            .session(&user.session_id)
            .body(Body::empty())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_bytes(resp).await;
        let frag = bytes_frag(&body);
        // There's NO nav and page frame, it's a fragment.
        assert!(!has_logged_in_nav(&frag));
        // Includes dogears list with all dogears (assumption: test user has 2)
        assert_eq!(frag.select(&sel("#dogears li")).count(), 2);
    }
    // Pagination: same as index
    {
        // page 2 size 1
        let req = new_req("GET", "/fragments/dogears?page=2&size=1")
            .session(&user.session_id)
            .body(Body::empty())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_bytes(resp).await;
        let frag = bytes_frag(&body);
        // dogears list present, has #size items
        assert_eq!(frag.select(&sel("#dogears li")).count(), 1);
        // pagination controls present: prev link, but no next link
        assert!(!frag.has(".pagination-link.pagination-next"));
        let prev = frag
            .select(&sel(".pagination-link.pagination-previous"))
            .next()
            .expect("must be present");
        // prev link goes to pg 1 w/ specified size
        assert_eq!(prev.attr("href").unwrap(), "/?page=1&size=1");
        // has correct fragment URL for in-place swap
        assert_eq!(
            prev.attr("data-fragment-url").unwrap(),
            "/fragments/dogears?page=1&size=1"
        );
    }
}

/// These are just web pages.
#[tokio::test]
async fn faq_and_install_test() {
    let state = test_state().await;
    let mut app = eardogger_app(state.clone());
    let user = state.db.test_user("whoever").await.unwrap();

    for &uri in &["/install", "/faq"] {
        // Works logged-out
        {
            let req = new_req("GET", uri).body(Body::empty()).unwrap();
            let resp = do_req(&mut app, req).await;
            assert_eq!(resp.status(), StatusCode::OK);
            let body = body_bytes(resp).await;
            let doc = bytes_doc(&body);
            assert!(!has_logged_in_nav(&doc));
        }
        // Works logged in
        {
            let req = new_req("GET", uri)
                .session(&user.session_id)
                .body(Body::empty())
                .unwrap();
            let resp = do_req(&mut app, req).await;
            assert_eq!(resp.status(), StatusCode::OK);
            let body = body_bytes(resp).await;
            let doc = bytes_doc(&body);
            assert!(has_logged_in_nav(&doc));
        }
    }
}
