use crate::util::{
    url_encoding::encode_uri_component, uuid_string, COOKIE_SESSION, DELETE_ACCOUNT_CONFIRM_STRING,
};

use super::app_tests::*;

const TEST_PASSWORD: &str = crate::db::Db::TEST_PASSWORD;

/// /public and /status
#[tokio::test]
async fn app_basics_noauth_test() {
    let state = test_state().await;
    let mut app = eardogger_app(state.clone());

    // Static file serving is hooked up right
    {
        let req = new_req("GET", "/public/style.css").empty();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_page_and_contains(resp, "--color-background").await;
    }

    // Status is hooked up right
    {
        let req = new_req("GET", "/status").empty();
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
        let req = new_req("GET", "/").token(&user.manage_token).empty();
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
        let req = new_req("GET", "/").empty();
        let resp = do_req(&mut app, req).await;
        assert_login_page(resp).await;
    }
    // No login: fragment serves 401
    {
        let req = new_req("GET", "/fragments/dogears").empty();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
    // Shared behaviors
    for &(uri, kind) in &[("/", HtmlKind::Doc), ("/fragments/dogears", HtmlKind::Frag)] {
        // Serves the dogears list.
        {
            let req = new_req("GET", uri).session(&user.session_id).empty();
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
            let req = new_req("GET", &with_q).session(&user.session_id).empty();
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
            let req = new_req("GET", &with_q).session(&user.session_id).empty();
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
            let req = new_req("GET", uri).empty();
            let resp = do_req(&mut app, req).await;
            assert_eq!(resp.status(), StatusCode::OK);
            let body = body_bytes(resp).await;
            let doc = bytes_doc(&body);
            assert!(!has_logged_in_nav(&doc));
        }
        // Works logged in
        {
            let req = new_req("GET", uri).session(&user.session_id).empty();
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
            let req = new_req("GET", uri).empty();
            let resp = do_req(&mut app, req).await;
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        }
        // Includes tokens list (hardcoded assumption: test user has 2.)
        {
            let req = new_req("GET", uri).session(&user.session_id).empty();
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
            let req = new_req("GET", with_query).session(&user.session_id).empty();
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

/// /mark/:url page displays one of two underlying pages: the "marked"
/// page if the URL matches an existing dogear, or the "create" page
/// if it doesn't.
#[tokio::test]
async fn mark_url_page_test() {
    let state = test_state().await;
    let mut app = eardogger_app(state.clone());
    let user = state.db.test_user("whoever").await.unwrap();

    // Matching existing dogear: shows marked page in slow mode
    {
        let req = new_req("GET", "/mark/https%3A%2F%2Fexample.com%2Fcomic%2F25")
            .session(&user.session_id)
            .empty();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_bytes(resp).await;
        let doc = bytes_doc(&body);
        // it's the marked page
        assert!(doc.has("#mark-success"));
        assert!(!doc.has("form#create-dogear"));
        // and it's in slow-mode
        assert!(doc.has("#slow-mode"));
    }
    // New site: shows create page
    {
        let req = new_req("GET", "/mark/https%3A%2F%2Fexample.com%2Fmanual%2F6")
            .session(&user.session_id)
            .empty();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_bytes(resp).await;
        let doc = bytes_doc(&body);
        // it's the create page
        assert!(doc.has("form#create-dogear"));
        assert!(!doc.has("#mark-success"));
    }
}

/// Like the mark page, the resume page can be two different things:
/// if you've got a dogear for the URL, it boots your ass out the door,
/// and if not it shows the create page.
#[tokio::test]
async fn resume_url_test() {
    let state = test_state().await;
    let mut app = eardogger_app(state.clone());
    let user = state.db.test_user("whoever").await.unwrap();

    // hardcoded assumption: we're on page 24 of the example comic.
    {
        let req = new_req("GET", "/resume/https%3A%2F%2Fexample.com%2Fcomic%2F10")
            .session(&user.session_id)
            .empty();
        let resp = do_req(&mut app, req).await;
        assert!(resp.status().is_redirection());
        let dest = resp
            .headers()
            .get(header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(dest, "https://example.com/comic/24");
    }
    // New site: shows create page
    {
        let req = new_req("GET", "/resume/https%3A%2F%2Fexample.com%2Fmanual%2F6")
            .session(&user.session_id)
            .empty();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_bytes(resp).await;
        let doc = bytes_doc(&body);
        // it's the create page
        assert!(doc.has("form#create-dogear"));
    }
}

/// And here's the result of USING the create form that the prior two routes
/// can return.
#[tokio::test]
async fn post_mark_test() {
    let state = test_state().await;
    let mut app = eardogger_app(state.clone());
    let user = state.db.test_user("whoever").await.unwrap();

    let form_without = |name: &str, current: &str, prefix: &str| {
        format!(
            "display_name={}&current={}&prefix={}",
            encode_uri_component(name),
            encode_uri_component(current),
            encode_uri_component(prefix),
        )
    };
    let form = |name: &str, current: &str, prefix: &str| {
        format!(
            "{}&csrf_token={}",
            form_without(name, current, prefix),
            &user.csrf_token
        )
    };

    // it's csrf-guarded
    {
        let form_body = form_without(
            "Manual",
            "https://example.com/manual/5",
            "example.com/manual/",
        );
        reusable_csrf_guard_test(&mut app, "/mark", &form_body, &user.session_id).await;
    }
    // Most of the validation is back in the db layer, so I'm not gonna bother re-testing.
    // happy path:
    {
        let form_body = form(
            "Manual",
            "https://example.com/manual/6",
            "example.com/manual",
        );
        let req = new_req("POST", "/mark")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .session(&user.session_id)
            .body(Body::from(form_body))
            .unwrap();
        let resp = do_req(&mut app, req).await;
        // TODO: Hmm, actually this should probably be Created, but I think it's 200.
        assert!(resp.status().is_success());
        let body = body_bytes(resp).await;
        let doc = bytes_doc(&body);
        // it's the marked page
        assert!(doc.has("#mark-success"));
        // and it's NOT in slow-mode
        assert!(!doc.has("#slow-mode"));
    }
}

/// Helper type for testing the login and signup routes, since they use a
/// different anti-csrf scheme that isn't tied to a user.
struct SignedLoginCsrf {
    uuid: String,
    signature: String,
}

impl SignedLoginCsrf {
    /// Fetch a page with the login form on it, and grab the login csrf cookie
    async fn request(app: &mut axum::Router) -> Self {
        let csrf_req = new_req("GET", "/").empty();
        let csrf_resp = do_req(app, csrf_req).await;
        Self::from_resp(csrf_resp)
    }

    /// Grab the csrf cookie out of a response
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
            TEST_PASSWORD, uuid, return_to
        )
    };

    // Grab a signed csrf token from the login form Set-Cookie header
    let valid_csrf = SignedLoginCsrf::request(&mut app).await;

    // Okay!!!
    // happy path: you get a session cookie and a redirect.
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
        let form_body = format!("username=whoever&password={}", TEST_PASSWORD);
        let req = new_req("POST", "/login")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(header::COOKIE, valid_csrf.to_cookie())
            .body(Body::from(form_body))
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert!(resp.status().is_client_error()); // any 4xx
    }
}

/// This is going to be mostly a copypasta of the login test, but the form is different
/// enough that it didn't make sense to deduplicate.
#[tokio::test]
async fn post_signup_test() {
    let state = test_state().await;
    let mut app = eardogger_app(state.clone());
    // no user this time!

    // Grab a signed csrf token from the login form Set-Cookie header
    let valid_csrf = SignedLoginCsrf::request(&mut app).await;

    // happy path: sessid cookie and a redirect.
    {
        let form = format!("new_username=somebody&new_password=aaaaa&new_password_again=aaaaa&email=&login_csrf_token={}", &valid_csrf.uuid);
        let req = new_req("POST", "/signup")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(header::COOKIE, valid_csrf.to_cookie())
            .body(Body::from(form))
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
        // got a sessid cookie
        let found_sessid = resp
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .any(|val| val.to_str().unwrap().starts_with(COOKIE_SESSION));
        assert!(found_sessid);
    }

    // We actually do have a case for 403-ing if you're signed in, but I'm
    // simply not attached enough to it to add a test.

    // Unhappy path: 400 if your csrf token doesn't match the cookie
    {
        let form = format!("new_username=somebody&new_password=aaaaa&new_password_again=aaaaa&email=&login_csrf_token={}", uuid_string());
        let req = new_req("POST", "/signup")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(header::COOKIE, valid_csrf.to_cookie())
            .body(Body::from(form))
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
    // Unhappy path: 4xx if you omit the csrf token completely
    // TODO: the form params deserialization handles this, so it skips the nice error page.
    // Maybe get around to wrapping the rejection one of these days.
    {
        let form = "new_username=somebody&new_password=aaaaa&new_password_again=aaaaa&email=";
        let req = new_req("POST", "/signup")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(header::COOKIE, valid_csrf.to_cookie())
            .body(Body::from(form))
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert!(resp.status().is_client_error()); // any 4xx
    }
}

/// Reusable test case for ensuring a form-urlencoded POST endpoint is
/// protected by session-derived CSRF token. Since the affected endpoint's
/// form body might be anything, caller's expected to construct it as needed
/// but leave the csrf token off. Also, we expect that every affected form
/// uses the name "csrf_token" for its csrf guard field.
async fn reusable_csrf_guard_test(
    app: &mut Router,
    uri: &str,
    form_body_minus_csrf: &str,
    sessid: &str,
) {
    // wrong csrf token: 400
    {
        let form = format!("{}&csrf_token={}", form_body_minus_csrf, uuid_string());
        let req = new_req("POST", uri)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .session(sessid)
            .body(Body::from(form))
            .unwrap();
        let resp = do_req(app, req).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
    // absent csrf token: 4xx of some kind (handled by the Form extractor.
    // TODO: wrap that rejection in WebError.)
    {
        let req = new_req("POST", uri)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .session(sessid)
            .body(Body::from(form_body_minus_csrf.to_string()))
            .unwrap();
        let resp = do_req(app, req).await;
        assert!(resp.status().is_client_error());
    }
}

/// Logout form logs you out, and is guarded by your session csrf token.
#[tokio::test]
async fn post_logout_test() {
    let state = test_state().await;
    let mut app = eardogger_app(state.clone());
    let user = state.db.test_user("whoever").await.unwrap();

    // Test unhappy path first, because... it'll destroy our session. :)
    reusable_csrf_guard_test(&mut app, "/logout", "", &user.session_id).await;
    // Happy path: redirect and a removal cookie.
    {
        let form = format!("csrf_token={}", &user.csrf_token);
        let req = new_req("POST", "/logout")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .session(&user.session_id)
            .body(Body::from(form))
            .unwrap();
        let resp = do_req(&mut app, req).await;
        // redirected... don't really care where tbh lol
        assert!(resp.status().is_redirection());
        // sessid removal:
        let got_empty_sessid_cookie =
            resp.headers()
                .get_all(header::SET_COOKIE)
                .iter()
                .any(|h_val| {
                    let (name, stuff) = h_val.to_str().unwrap().split_once('=').unwrap();
                    if name == COOKIE_SESSION {
                        let (val, _opts) = stuff.split_once(';').unwrap();
                        if val.is_empty() {
                            return true;
                        }
                    }
                    false
                });
        assert!(got_empty_sessid_cookie);
    }
}

#[tokio::test]
async fn post_change_password_test() {
    let state = test_state().await;
    let mut app = eardogger_app(state.clone());
    let user = state.db.test_user("whoever").await.unwrap();

    let form = |old: &str, new: &str, again: &str| {
        format!(
            "password={}&new_password={}&new_password_again={}&csrf_token={}",
            old, new, again, &user.csrf_token
        )
    };

    // csrf guard
    {
        let form = format!(
            "password={}&new_password={1}&new_password_again={1}",
            TEST_PASSWORD, "snthsnth"
        );
        reusable_csrf_guard_test(&mut app, "/changepassword", &form, &user.session_id).await;
    }
    // wrong password
    {
        let req = new_req("POST", "/changepassword")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .session(&user.session_id)
            .body(Body::from(form("blah", "snth", "snth")))
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = body_bytes(resp).await;
        let doc = bytes_doc(&body);
        assert!(doc.has("#error-page"));
    }
    // mismatched new passwords
    {
        let req = new_req("POST", "/changepassword")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .session(&user.session_id)
            .body(Body::from(form(TEST_PASSWORD, "snth", "htns")))
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = body_bytes(resp).await;
        let doc = bytes_doc(&body);
        assert!(doc.has("#error-page"));
    }
    // happy path
    {
        let req = new_req("POST", "/changepassword")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .session(&user.session_id)
            .body(Body::from(form(TEST_PASSWORD, "snth", "snth")))
            .unwrap();
        let resp = do_req(&mut app, req).await;
        // is redirect, don't really care where
        assert!(resp.status().is_redirection());
    }
}

#[tokio::test]
async fn post_change_email_test() {
    let state = test_state().await;
    let mut app = eardogger_app(state.clone());
    let user = state.db.test_user("whoever").await.unwrap();

    let form = |pw: &str, email: &str| {
        format!(
            "password={}&new_email={}&csrf_token={}",
            pw, email, &user.csrf_token
        )
    };
    // csrf guard
    {
        let form = format!(
            "password={}&new_email={}",
            TEST_PASSWORD, "whenever@example.com"
        );
        reusable_csrf_guard_test(&mut app, "/change_email", &form, &user.session_id).await;
    }
    // 400 on bad password
    {
        let req = new_req("POST", "/change_email")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .session(&user.session_id)
            .body(Body::from(form("uehtoans", "whenever@example.com")))
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = body_bytes(resp).await;
        let doc = bytes_doc(&body);
        assert!(doc.has("#error-page"));
    }
    // happy path: redirect
    {
        let req = new_req("POST", "/change_email")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .session(&user.session_id)
            .body(Body::from(form(TEST_PASSWORD, "whenever@example.com")))
            .unwrap();
        let resp = do_req(&mut app, req).await;
        // don't really care where
        assert!(resp.status().is_redirection());
    }
}

#[tokio::test]
async fn post_delete_account_test() {
    let state = test_state().await;
    let mut app = eardogger_app(state.clone());
    let user = state.db.test_user("whoever").await.unwrap();

    let form = |pw: &str, please: &str| {
        format!(
            "password={}&confirm_delete_account={}&csrf_token={}",
            pw, please, &user.csrf_token
        )
    };
    // csrf guard
    {
        let form = format!(
            "password={}&confirm_delete_account={}",
            TEST_PASSWORD, DELETE_ACCOUNT_CONFIRM_STRING
        );
        reusable_csrf_guard_test(&mut app, "/delete_account", &form, &user.session_id).await;
    }
    // 400 on bad password
    {
        let req = new_req("POST", "/delete_account")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .session(&user.session_id)
            .body(Body::from(form("uehtoans", DELETE_ACCOUNT_CONFIRM_STRING)))
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = body_bytes(resp).await;
        let doc = bytes_doc(&body);
        assert!(doc.has("#error-page"));
    }
    // 400 on bad confirm string
    {
        let req = new_req("POST", "/delete_account")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .session(&user.session_id)
            .body(Body::from(form(TEST_PASSWORD, "dewete my account uwu")))
            .unwrap();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = body_bytes(resp).await;
        let doc = bytes_doc(&body);
        assert!(doc.has("#error-page"));
    }
    // happy path: die
    {
        let req = new_req("POST", "/delete_account")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .session(&user.session_id)
            .body(Body::from(form(
                TEST_PASSWORD,
                DELETE_ACCOUNT_CONFIRM_STRING,
            )))
            .unwrap();
        let resp = do_req(&mut app, req).await;
        // don't care where
        assert!(resp.status().is_redirection());
    }
}

#[tokio::test]
async fn delete_token_test() {
    let state = test_state().await;
    let mut app = eardogger_app(state.clone());
    let user = state.db.test_user("whoever").await.unwrap();

    // btw: DELETEs aren't plain posts, so they're not CSRF-vulnerable.
    // gotta grab one of these tokens out the DB, since we need its ID.
    let (manage_token, _) = state
        .db
        .tokens()
        .authenticate(&user.manage_token)
        .await
        .unwrap()
        .unwrap();
    // 404 on whiff
    {
        let req = new_req("DELETE", "/tokens/999")
            .session(&user.session_id)
            .empty();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
    // 204 on hit
    {
        let req = new_req("DELETE", format!("/tokens/{}", manage_token.id))
            .session(&user.session_id)
            .empty();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }
}

/// This is a bit odd because it's a "plain" POST request, but the body
/// is empty and the csrf token comes in via query param. This is because
/// it's coming in via the fragment-replacer javascript. I might consider
/// reworking that someday.
#[tokio::test]
async fn post_fragment_personalmark_test() {
    let state = test_state().await;
    let mut app = eardogger_app(state.clone());
    let user = state.db.test_user("whoever").await.unwrap();

    let uri = |csrf: &str| format!("/fragments/personalmark?csrf_token={}", csrf);
    // Gotta do the csrf test manually.
    // wrong csrf token:
    {
        let req = new_req("POST", uri(&uuid_string()))
            .session(&user.session_id)
            .empty();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = body_bytes(resp).await;
        let doc = bytes_doc(&body);
        assert!(doc.has("#error-page"));
    }
    // absent csrf token:
    {
        let req = new_req("POST", "/fragments/personalmark")
            .session(&user.session_id)
            .empty();
        let resp = do_req(&mut app, req).await;
        // TODO: wrap rejection type for Query
        assert!(resp.status().is_client_error());
    }
    // happy path:
    {
        let req = new_req("POST", uri(&user.csrf_token))
            .session(&user.session_id)
            .empty();
        let resp = do_req(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = body_bytes(resp).await;
        let frag = bytes_frag(&body);
        // One selector from the fragment, one from the inner macro call.
        assert!(frag.has("#generate-personal-bookmarklet-fragment .bookmarklet"));
    }
}
