use crate::util::{uuid_string, COOKIE_SESSION};

use super::app_tests::*;

/// /public and /status
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

/// AuthSession extractor is properly hooked up: Providing a token is
/// the same as not being logged in at all, for routes that take an
/// AuthSession rather than an AuthAny. This is the only time I'll
/// test this, other routes can just trust the type assurances.
#[tokio::test]
async fn token_isnt_logged_in() {
    let state = test_state().await;
    let mut app = eardogger_app(state.clone());
    let user = state.db.test_user("whoever").await.unwrap();

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

struct SignedLoginCsrf {
    uuid: String,
    signature: String,
}

impl SignedLoginCsrf {
    fn from_resp(resp: Response<Body>) -> Self {
        // grab first available cookie and crack it apart...
        // this is highly yolo maneuvering but whatever lol
        let cookie_str = resp
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap();
        // We want the part after the first "=" but before the first ";".
        let cookie_val = cookie_str
            .split_once('=')
            .unwrap()
            .1
            .split_once(';')
            .unwrap()
            .0;
        // Then, there's another equals sign that seems to divide the signature
        // and the UUID.
        let (s, u) = cookie_val.split_once('=').unwrap();
        Self {
            uuid: u.to_string(),
            signature: s.to_string(),
        }
    }

    fn to_cookie(&self) -> String {
        format!(
            "{}={}={}",
            crate::util::COOKIE_LOGIN_CSRF,
            self.signature,
            self.uuid
        )
    }

    // ...and then form fields just get the plain uuid.
}

#[tokio::test]
async fn post_login_test() {
    let state = test_state().await;
    let mut app = eardogger_app(state.clone());
    let _user = state.db.test_user("whoever").await.unwrap();

    // quite a bit of nasty setup for this one.

    // form-urlencoded for body
    let form = |uuid: &str, return_to: &str| {
        format!(
            "username=whoever&password={}&login_csrf_token={}&return_to={}",
            crate::db::Db::TEST_PASSWORD,
            uuid,
            return_to
        )
    };

    // Grab a signed csrf token from the login form Set-Cookie header
    let valid_csrf = {
        let csrf_req = new_req("GET", "/").body(Body::empty()).unwrap();
        let csrf_resp = do_req(&mut app, csrf_req).await;
        // grab first cookie and crack it apart... this is pretty yolo maneuvering but w/e.
        SignedLoginCsrf::from_resp(csrf_resp)
    };

    // Okay!!!
    // happy path: you get a session cookie and a default redirect to /.
    {
        let req = new_req("POST", "/login")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(header::COOKIE, valid_csrf.to_cookie())
            .body(Body::from(form(&valid_csrf.uuid, "/")))
            .unwrap();
        let resp = do_req(&mut app, req).await;
        // got redirected
        assert!(resp.status().is_redirection());
        // to the expected location
        let return_to = resp
            .headers()
            .get(header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(
            return_to.trim_start_matches(&state.config.public_url.origin().ascii_serialization()),
            "/"
        );
        // got a sessid cookie... need to use .any because we also send a
        // removal cookie to waste the login csrf.
        let found_sessid = resp
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .any(|val| val.to_str().unwrap().starts_with(COOKIE_SESSION));
        assert!(found_sessid);
    }

    // Irrelevant path: Apparently we don't care if you're already logged in. :shrug:

    // Unhappy path: 400 if your csrf token doesn't match the cookie
    {
        let req = new_req("POST", "/login")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(header::COOKIE, valid_csrf.to_cookie())
            .body(Body::from(form(&uuid_string(), "/")))
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
    // Unhappy path: 4xx if you omit the csrf token completely
    // TODO: the form params deserialization handles this, so it skips the nice error page.
    // Maybe get around to wrapping the rejection one of these days.
    {
        let form_body = format!("username=whoever&password={}", crate::db::Db::TEST_PASSWORD);
        let req = new_req("POST", "/login")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(header::COOKIE, valid_csrf.to_cookie())
            .body(Body::from(form_body))
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert!(resp.status().is_client_error()); // any 4xx
    }
}