use super::state::DogState;
use super::templates::*;
use super::web_result::{WebError, WebResult};
use crate::util::{uuid_string, COOKIE_LOGIN_CSRF};

use axum::{
    extract::{Form, State},
    http::StatusCode,
    response::{Html, Redirect},
};
use minijinja::context;
use serde::Deserialize;
use tower_cookies::{Cookie, Cookies};

#[derive(Deserialize)]
struct LoginParams {
    username: String,
    password: String,
    login_csrf_token: String,
    return_to: String,
}

/// Handle POSTs from the login form. The login form can appear on multiple pages,
/// and includes a pointer back to the page the user was originally trying to reach.
/// The form also includes a random token to be used in a signed double-submit
/// CSRF-prevention scheme; compare it to the cookie to validate that the post came
/// from a real login form, not a remote-site forgery.
async fn post_login(
    State(state): State<DogState>,
    cookies: Cookies,
    Form(params): Form<LoginParams>,
) -> WebResult<Redirect> {
    // First, check the login CSRF cookie
    let signed_cookies = cookies.signed(&state.cookie_key);
    let Some(csrf_cookie) = signed_cookies.get(COOKIE_LOGIN_CSRF) else {
        return Err(WebError::new(
            StatusCode::BAD_REQUEST,
            r#"The login form you tried to use was broken.
                Go back to the home page and try logging in again."#
                .to_string(),
        ));
    };
    if csrf_cookie.value() != params.login_csrf_token {
        return Err(WebError::new(
            StatusCode::BAD_REQUEST,
            r#"The login form you tried to use was stale or had been
                tampered with. Go back to the home page and try logging in again."#
                .to_string(),
        ));
    }
    // Cool. 👍🏼 Waste the cookie, it's spent.
    signed_cookies.remove(csrf_cookie);

    // Sort out the redirect URL. If it's bad (illegible, off-site...),
    // just go to the home page.
    let mut redirect_to = state
        .config
        .own_origin
        .join(&params.return_to)
        .unwrap_or_else(|_| state.config.own_origin.clone());
    if redirect_to.origin() != state.config.own_origin.origin() {
        redirect_to = state.config.own_origin.clone();
    }

    // then, authenticate user and tack on a session cookie.
    if let Some(user) = state
        .db
        .users()
        .authenticate(&params.username, &params.password)
        .await?
    {
        let session = state.db.sessions().create(user.id).await?;
        cookies.add(session.into_cookie());
    }

    // Finally, redirect. If the login failed, this will just show the login page again.
    // TODO: I want to propagate the "last failed state" if you end up
    // redirecting and then it shows the login page again, but I'm still
    // mulling how to do that reliably. First thing that occurred to me was
    // a query param, but I don't love it. Guess I could use a cookie too :thonk:
    Ok(Redirect::to(redirect_to.as_str()))
}

/// Render the login form, including the anti-CSRF double-submit cookie.
/// Notably, this is NOT a Handler fn! Since many routes can fall back
/// to the login form, the idea is to just return an awaited call to
/// login_form if they hit that branch.
async fn login_form(state: DogState, cookies: Cookies, return_to: &str) -> WebResult<Html<String>> {
    let csrf_token = uuid_string();
    // Render the html string first, so we can get some use out of the owned string
    // before consuming it to build the cookie. 👍🏼
    let login_page = LoginPage {
        return_to,
        previously_failed: false, // TODO
    };
    let common = Common {
        title: "Welcome to Eardogger",
        user: None,
        csrf_token: &csrf_token,
    };
    let ctx = context! { login_page, common };
    let page = state.render_view("login.html.j2", ctx)?;

    // no expires (session cookie)
    // no http_only (owasp says don't?)
    let csrf_cookie = Cookie::build((COOKIE_LOGIN_CSRF, csrf_token))
        .secure(true)
        .same_site(tower_cookies::cookie::SameSite::Strict)
        .build()
        .into_owned();
    cookies.signed(&state.cookie_key).add(csrf_cookie);

    Ok(Html(page))
}