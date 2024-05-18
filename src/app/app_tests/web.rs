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

/// Test the index (/) and /fragments/dogears, which have several similar behaviors.
#[tokio::test]
async fn index_and_dogears_test() {
    let state = test_state().await;
    let mut app = eardogger_app(state.clone());
    let user = state.db.test_user("whoever").await.unwrap();

    // No login: index serves login page
    {
        let req = new_req("GET", "/").body(Body::empty()).unwrap();
        let resp = do_req(&mut app, req).await;
        assert_login_page(resp).await;
    }
    // No login: fragment serves 401
    {
        let req = new_req("GET", "/fragments/dogears")
            .body(Body::empty())
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
    // Shared behaviors
    for &(uri, kind) in &[("/", HtmlKind::Doc), ("/fragments/dogears", HtmlKind::Frag)] {
        // Serves the dogears list.
        {
            let req = new_req("GET", uri)
                .session(&user.session_id)
                .body(Body::empty())
                .unwrap();
            let resp = do_req(&mut app, req).await;
            assert_eq!(resp.status(), StatusCode::OK);
            let body = body_bytes(resp).await;
            let html = bytes_html(&body, kind);
            // It's either a fragment or a whole page:
            match kind {
                HtmlKind::Doc => {
                    // Includes main layout content for a logged-in user:
                    assert!(has_logged_in_nav(&html));
                    // includes "manual mode" form, for now
                    assert!(html.has("form#update-dogear"));
                }
                HtmlKind::Frag => {
                    // No page frame
                    assert!(!has_logged_in_nav(&html));
                }
            }
            // Includes dogears list with all dogears (assumption: test user has 2)
            assert_eq!(html.select(&sel("#dogears li")).count(), 2);
        }
        // pagination. Assumption: test user has two dogears.
        {
            let with_q = format!("{}?size=1", uri);
            let req = new_req("GET", &with_q)
                .session(&user.session_id)
                .body(Body::empty())
                .unwrap();
            let resp = do_req(&mut app, req).await;
            assert_eq!(resp.status(), StatusCode::OK);
            let body = body_bytes(resp).await;
            let html = bytes_html(&body, kind);
            // dogears list present, has #size items
            assert_eq!(html.select(&sel("#dogears li")).count(), 1);
            // pagination controls present: next link, but no prev link
            assert!(!html.has(".pagination-link.pagination-previous"));
            let next = html
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
            let with_q = format!("{}?page=2&size=1", uri);
            let req = new_req("GET", &with_q)
                .session(&user.session_id)
                .body(Body::empty())
                .unwrap();
            let resp = do_req(&mut app, req).await;
            let body = body_bytes(resp).await;
            let html = bytes_html(&body, kind);
            // dogears list present, has #size items
            assert_eq!(html.select(&sel("#dogears li")).count(), 1);
            // pagination controls present: prev link, but no next link
            assert!(!html.has(".pagination-link.pagination-next"));
            let prev = html
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

/// Very similar to index page, w/ the pagination.
#[tokio::test]
async fn account_and_tokens_test() {
    let state = test_state().await;
    let mut app = eardogger_app(state.clone());
    let user = state.db.test_user("whoever").await.unwrap();

    // Shared behaviors
    for &(uri, kind) in &[
        ("/account", HtmlKind::Doc),
        ("/fragments/tokens", HtmlKind::Frag),
    ] {
        // Logged out: 401
        {
            let req = new_req("GET", uri).body(Body::empty()).unwrap();
            let resp = do_req(&mut app, req).await;
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        }
        // Includes tokens list (hardcoded assumption: test user has 2.)
        {
            let req = new_req("GET", uri)
                .session(&user.session_id)
                .body(Body::empty())
                .unwrap();
            let resp = do_req(&mut app, req).await;
            assert_eq!(resp.status(), StatusCode::OK);
            let body = body_bytes(resp).await;
            let html = bytes_html(&body, kind);
            // It's a fragment OR a full page, depending
            match kind {
                HtmlKind::Doc => {
                    assert!(has_logged_in_nav(&html));
                    // also it's got various forms
                    assert!(html.has("form#changepasswordform"));
                    assert!(html.has("form#change_email_form"));
                    assert!(html.has("form#delete_account_form"));
                }
                HtmlKind::Frag => {
                    assert!(!has_logged_in_nav(&html));
                }
            }
            // 2 tokens
            let tokens = html.select(&sel("#tokens-list .token")).count();
            assert_eq!(tokens, 2);
        }
        // Pagination
        {
            let with_query = format!("{}?size=1&page=2", uri);
            let req = new_req("GET", with_query)
                .session(&user.session_id)
                .body(Body::empty())
                .unwrap();
            let resp = do_req(&mut app, req).await;
            assert_eq!(resp.status(), StatusCode::OK);
            let body = body_bytes(resp).await;
            let html = bytes_html(&body, kind);
            // Has #size items
            let tokens = html.select(&sel("#tokens-list .token")).count();
            assert_eq!(tokens, 1);
            // Has no next link (we're on final page)
            assert!(!html.has(".pagination-link.pagination-next"));
            // Has prev link
            let prev = html
                .select(&sel(".pagination-link.pagination-previous"))
                .next()
                .expect("gotta have it");
            // Links to account page w/ specified page size and #page - 1
            assert_eq!(prev.attr("href").unwrap(), "/account?page=1&size=1");
            // has equivalent fragment URL for js-nav
            assert_eq!(
                prev.attr("data-fragment-url").unwrap(),
                "/fragments/tokens?page=1&size=1"
            );
        }
    }
}
