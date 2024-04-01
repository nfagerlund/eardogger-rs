use super::authentication::AuthSession;
use super::state::DogState;
use super::templates::*;
use super::web_result::{WebError, WebResult};
use crate::db::TokenScope;
use crate::util::{
    check_new_password, uuid_string, COOKIE_LOGIN_CSRF, COOKIE_SESSION, PAGE_DEFAULT_SIZE,
    SHORT_DATE,
};

use axum::extract::Path;
use axum::{
    extract::{Form, Query, State},
    http::{StatusCode, Uri},
    response::{Html, Redirect},
};
use minijinja::context;
use serde::Deserialize;
use time::OffsetDateTime;
use tower_cookies::{Cookie, Cookies};
use tracing::error;

#[derive(Deserialize, Debug)]
pub struct PaginationQuery {
    page: Option<u32>,
    size: Option<u32>,
}

impl PaginationQuery {
    /// Getter w/ default value
    pub fn page(&self) -> u32 {
        self.page.unwrap_or(1)
    }
    /// Getter w/ default value
    pub fn size(&self) -> u32 {
        self.size.unwrap_or(PAGE_DEFAULT_SIZE)
    }
}

/// The void!!!!!
#[tracing::instrument]
pub async fn four_oh_four() -> WebError {
    WebError::new(StatusCode::NOT_FOUND, "Well I tried, but 404".to_string())
}
#[tracing::instrument]
pub async fn status() -> StatusCode {
    StatusCode::NO_CONTENT
}

/// The home page! Shows your dogears list if logged in, and the login
/// form if not.
#[tracing::instrument]
pub async fn root(
    State(state): State<DogState>,
    Query(query): Query<PaginationQuery>,
    maybe_auth: Option<AuthSession>,
    // for login form:
    uri: Uri,
    cookies: Cookies,
) -> WebResult<Html<String>> {
    // Branch to login form, maybe
    let Some(auth) = maybe_auth else {
        let path = uri.to_string();
        return login_form(state, cookies, &path).await;
    };

    let (dogears, meta) = state
        .db
        .dogears()
        .list(auth.user.id, query.page(), query.size())
        .await?;
    let title = format!("{}'s Dogears", &auth.user.username);

    let common = auth.common_args(&title);
    let dogears_list = DogearsList {
        dogears: &dogears,
        pagination: meta.to_pagination(),
    };
    let ctx = context! {common, dogears_list};

    Ok(Html(state.render_view("index.html.j2", ctx)?))
}

/// Kind of like the index page, except 1. no login form, 2. therefore auth required.
#[tracing::instrument]
pub async fn fragment_dogears(
    State(state): State<DogState>,
    Query(query): Query<PaginationQuery>,
    auth: AuthSession,
) -> WebResult<Html<String>> {
    let (dogears, meta) = state
        .db
        .dogears()
        .list(auth.user.id, query.page(), query.size())
        .await?;
    let dogears_list = DogearsList {
        dogears: &dogears,
        pagination: meta.to_pagination(),
    };
    let ctx = context! {dogears_list};
    Ok(Html(state.render_view("fragment.dogears.html.j2", ctx)?))
}

/// The mark-some-url page. One of:
/// - Updating existing dogear in slowmode (countdown to redirect).
/// - Create new dogear from URL we haven't seen before.
/// Can fall back to login page on logged out.
#[tracing::instrument]
pub async fn mark_url(
    State(state): State<DogState>,
    maybe_auth: Option<AuthSession>,
    cookies: Cookies,
    own_uri: Uri,
    Path(url): Path<String>,
) -> WebResult<Html<String>> {
    let Some(auth) = maybe_auth else {
        let path = own_uri.to_string();
        return login_form(state, cookies, &path).await;
    };
    let dogears = state.db.dogears();
    match dogears.update(auth.user.id, &url).await? {
        Some(res) => {
            let marked_page = MarkedPage {
                updated_dogears: &res,
                bookmarked_url: &url,
                slowmode: true,
            };
            let common = auth.common_args("Saved your place");
            let ctx = context! {marked_page, common};
            Ok(Html(state.render_view("marked.html.j2", ctx)?))
        }
        None => {
            let create_page = CreatePage {
                bookmarked_url: &url,
            };
            let common = auth.common_args("Dogear this?");
            let ctx = context! {create_page, common};
            Ok(Html(state.render_view("create.html.j2", ctx)?))
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateParams {
    display_name: Option<String>,
    current: String,
    prefix: String,
    csrf_token: String,
}

/// A POST to the /mark form, which is displayed on the create page. This
/// creates a new dogear, then displays the marked page (non-slowmode).
#[tracing::instrument]
pub async fn post_mark(
    State(state): State<DogState>,
    auth: AuthSession,
    Form(params): Form<CreateParams>,
) -> WebResult<Html<String>> {
    if params.csrf_token != auth.session.csrf_token {
        return Err(WebError {
            message: r#"The create-new-dogear form was stale or mangled.
                Go back, refresh that page, and try marking your spot again."#
                .to_string(),
            status: StatusCode::BAD_REQUEST,
        });
    }

    let res = state
        .db
        .dogears()
        .create(
            auth.user.id,
            &params.prefix,
            &params.current,
            params.display_name.as_deref(),
        )
        .await?;
    let marked_page = MarkedPage {
        updated_dogears: &[res],
        bookmarked_url: &params.current,
        slowmode: false,
    };
    let common = auth.common_args("Saved your place");
    let ctx = context! {marked_page, common};
    Ok(Html(state.render_view("marked.html.j2", ctx)?))
}

/// Display the faq/news/about page. This is almost a static page, but
/// if there's a user around, we want them for the layout header.
#[tracing::instrument]
pub async fn faq(
    State(state): State<DogState>,
    maybe_auth: Option<AuthSession>,
) -> WebResult<Html<String>> {
    let title = "About Eardogger";
    let common = match maybe_auth {
        Some(ref auth) => auth.common_args(title),
        None => Common::anonymous(title),
    };
    let ctx = context! {common};
    Ok(Html(state.render_view("faq.html.j2", ctx)?))
}

// Due to how I'm handling fragment fetch POSTs in the client-side JS,
// this comes in as query params rather than a form-urlencoded body.
#[derive(Debug, Deserialize)]
pub struct PersonalMarkParams {
    csrf_token: String,
}

#[tracing::instrument]
pub async fn post_fragment_personalmark(
    State(state): State<DogState>,
    auth: AuthSession,
    Query(params): Query<PersonalMarkParams>,
) -> WebResult<(StatusCode, Html<String>)> {
    if params.csrf_token != auth.session.csrf_token {
        return Err(WebError {
            message: r#"The bookmarklet generate button was stale or mangled.
                Refresh the page and try generating again."#
                .to_string(),
            status: StatusCode::BAD_REQUEST,
        });
    }
    // Skip an alloc w/ format_into:
    let mut comment_bytes: Vec<u8> = "Personal bookmarklet created ".into();
    OffsetDateTime::now_utc().format_into(&mut comment_bytes, SHORT_DATE)?;
    let comment = String::from_utf8(comment_bytes)?;
    // New token:
    let (_, token_cleartext) = state
        .db
        .tokens()
        .create(auth.user.id, TokenScope::WriteDogears, Some(&comment))
        .await?;
    // Build bookmarklet URL:
    let bookmarklet_url = state.render_bookmarklet("mark.js.j2", Some(&token_cleartext))?;
    // Render html fragment:
    let personal_mark = PersonalMark {
        bookmarklet_url: &bookmarklet_url,
    };
    let ctx = context! { personal_mark };
    Ok((
        StatusCode::CREATED,
        Html(state.render_view("fragment.personalmark.html.j2", ctx)?),
    ))
}

#[tracing::instrument]
pub async fn install(
    State(state): State<DogState>,
    maybe_auth: Option<AuthSession>,
) -> WebResult<Html<String>> {
    let title = "Install";
    let common = match maybe_auth {
        Some(ref auth) => auth.common_args(title),
        None => Common::anonymous(title),
    };
    let where_was = state.render_bookmarklet("where.js.j2", None)?;
    let install_page = InstallPage {
        where_was_i_bookmarklet_url: &where_was,
    };
    let ctx = context! { common, install_page };
    Ok(Html(state.render_view("install.html.j2", ctx)?))
}

/// The account page. Requires logged-in.
#[tracing::instrument]
pub async fn account(
    State(state): State<DogState>,
    auth: AuthSession,
    Query(query): Query<PaginationQuery>,
) -> WebResult<Html<String>> {
    let (tokens, meta) = state
        .db
        .tokens()
        .list(auth.user.id, query.page(), query.size())
        .await?;
    let common = auth.common_args("Manage account");
    let tokens_list = TokensList {
        tokens: &tokens,
        pagination: meta.to_pagination(),
    };
    let ctx = context! {common, tokens_list};
    Ok(Html(state.render_view("account.html.j2", ctx)?))
}

/// Kind of like the account page.
#[tracing::instrument]
pub async fn fragment_tokens(
    State(state): State<DogState>,
    auth: AuthSession,
    Query(query): Query<PaginationQuery>,
) -> WebResult<Html<String>> {
    let (tokens, meta) = state
        .db
        .tokens()
        .list(auth.user.id, query.page(), query.size())
        .await?;
    let tokens_list = TokensList {
        tokens: &tokens,
        pagination: meta.to_pagination(),
    };
    let ctx = context! {tokens_list};
    Ok(Html(state.render_view("fragment.tokens.html.j2", ctx)?))
}

/// Handle DELETE for tokens. Effectively an API method, but since it's
/// only valid for session users, it lives outside the api namespace.
#[tracing::instrument]
pub async fn delete_token(
    State(state): State<DogState>,
    auth: AuthSession,
    Path(id): Path<i64>,
) -> StatusCode {
    match state.db.tokens().destroy(id, auth.user.id).await {
        Ok(Some(_)) => StatusCode::NO_CONTENT,       // success
        Ok(None) => StatusCode::NOT_FOUND,           // failure
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR, // db splode
    }
}

/// Handle POSTs from the logout button. This redirects to /.
#[tracing::instrument]
pub async fn post_logout(
    State(state): State<DogState>,
    auth: AuthSession,
    cookies: Cookies,
    Form(params): Form<LogoutParams>,
) -> WebResult<Redirect> {
    // Destroy the session! Destroy the cookie! Well, first check the csrf token.
    if params.csrf_token != auth.session.csrf_token {
        return Err(WebError::new(
            StatusCode::BAD_REQUEST,
            r#"Something was wrong with that log out button! Go back to the
                home page and try logging out again."#
                .to_string(),
        ));
    }
    // Session goes first; that way if something goes wrong and it's still alive,
    // the user still has a cookie to try logging out with later.
    let res = state.db.sessions().destroy(&auth.session.id).await?;
    if res.is_none() {
        error!(
            logout.sessid = %auth.session.id,
            logout.userid = %auth.user.id,
            "Session not found for logout. This should be impossible, since we had a valid session!"
        );
    }
    cookies.remove((COOKIE_SESSION, "").into());
    Ok(Redirect::to("/"))
}

#[derive(Deserialize, Debug)]
pub struct LogoutParams {
    pub csrf_token: String,
}

#[derive(Deserialize, Debug)]
pub struct LoginParams {
    pub username: String,
    pub password: String,
    pub login_csrf_token: String,
    pub return_to: String,
}

/// Handle POSTs from the login form. The login form can appear on multiple pages,
/// and includes a pointer back to the page the user was originally trying to reach.
/// The form also includes a random token to be used in a signed double-submit
/// CSRF-prevention scheme; compare it to the cookie to validate that the post came
/// from a real login form, not a remote-site forgery.
#[tracing::instrument]
pub async fn post_login(
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

#[derive(Deserialize, Debug)]
pub struct SignupParams {
    new_username: String,
    new_password: String,
    new_password_again: String,
    // Really this'll always be present (and maybe blank), but downstream
    // recipients expect an Option and will flatmap it to normalize.
    email: Option<String>,
    login_csrf_token: String,
}

/// Handle POSTs from the signup form. This always appears alongside the login form.
/// It has some of the same properties, but it always redirects to /.
#[tracing::instrument]
pub async fn post_signup(
    State(state): State<DogState>,
    cookies: Cookies,
    maybe_auth: Option<AuthSession>,
    Form(params): Form<SignupParams>,
) -> WebResult<Redirect> {
    // First, check the login CSRF cookie
    let signed_cookies = cookies.signed(&state.cookie_key);
    let Some(csrf_cookie) = signed_cookies.get(COOKIE_LOGIN_CSRF) else {
        return Err(WebError::new(
            StatusCode::BAD_REQUEST,
            r#"The signup form you tried to use was broken.
                Go back to the home page and try signing up again."#
                .to_string(),
        ));
    };
    if csrf_cookie.value() != params.login_csrf_token {
        return Err(WebError::new(
            StatusCode::BAD_REQUEST,
            r#"The signup form you tried to use was stale or had been
                tampered with. Go back to the home page and try signing up again."#
                .to_string(),
        ));
    }
    // Cool. 👍🏼 Waste the cookie, it's spent.
    signed_cookies.remove(csrf_cookie);

    if maybe_auth.is_some() {
        return Err(WebError {
            message:
                "Can't sign up while you're still logged in. (How'd you even do that, by the way?)"
                    .to_string(),
            status: StatusCode::FORBIDDEN,
        });
    }
    if let Err(e) = check_new_password(&params.new_password, &params.new_password_again) {
        return Err(WebError {
            message: e.to_string(),
            status: StatusCode::BAD_REQUEST,
        });
    }
    let user = state
        .db
        .users()
        .create(
            &params.new_username,
            &params.new_password,
            params.email.as_deref(),
        )
        .await?;
    let session = state.db.sessions().create(user.id).await?;
    cookies.add(session.into_cookie());
    Ok(Redirect::to("/"))
}

/// Change password form args
#[derive(Deserialize, Debug)]
pub struct ChangePasswordParams {
    password: String,
    new_password: String,
    new_password_again: String,
    csrf_token: String,
}

/// The change password form, on the account page. Acts a little like the signup form.
#[tracing::instrument]
pub async fn post_changepassword(
    State(state): State<DogState>,
    auth: AuthSession,
    Form(params): Form<ChangePasswordParams>,
) -> WebResult<Redirect> {
    if params.csrf_token != auth.session.csrf_token {
        return Err(WebError {
            message: r#"The change password form you tried to use was stale, or
                had been tampered with. Go back to the account page and try
                changing your password again."#
                .to_string(),
            status: StatusCode::BAD_REQUEST,
        });
    }
    if let Err(e) = check_new_password(&params.new_password, &params.new_password_again) {
        return Err(WebError {
            message: e.to_string(),
            status: StatusCode::BAD_REQUEST,
        });
    }
    let users = state.db.users();
    let Some(user) = users
        .authenticate(&auth.user.username, &params.password)
        .await?
    else {
        return Err(WebError {
            message: "Wrong existing password".to_string(),
            status: StatusCode::BAD_REQUEST,
        });
    };
    users
        .set_password(&user.username, &params.new_password)
        .await?;

    Ok(Redirect::to("/account?changed=password"))
}

/// Render the login form, including the anti-CSRF double-submit cookie.
/// Notably, this is NOT a Handler fn! Since many routes can fall back
/// to the login form, the idea is to just return an awaited call to
/// login_form if they hit that branch.
#[tracing::instrument]
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
