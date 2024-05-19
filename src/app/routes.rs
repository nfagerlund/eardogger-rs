use super::authentication::{AuthAny, AuthSession};
use super::state::DogState;
use super::templates::*;
use super::web_result::{ApiError, ApiResult, WebError, WebResult};
use crate::db::{Dogear, TokenScope};
use crate::util::{
    check_new_password, clean_optional_form_field, uuid_string, ListMeta, Pagination, UserError,
    COOKIE_LOGIN_CSRF, COOKIE_SESSION, DELETE_ACCOUNT_CONFIRM_STRING, PAGE_DEFAULT_SIZE,
    SHORT_DATE,
};

use axum::extract::Path;
use axum::{
    extract::{Form, Query, State},
    http::{StatusCode, Uri},
    response::{Html, IntoResponse, Json, Redirect, Response},
};
use http::{header, HeaderMap, HeaderValue};
use minijinja::context;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tower_cookies::{Cookie, Cookies};
use tracing::error;
use url::Url;

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
#[tracing::instrument(skip_all)]
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
#[tracing::instrument(skip_all)]
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
#[tracing::instrument(skip_all)]
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
    // Dogears::create will normalize the Some("") case.
    display_name: Option<String>,
    current: String,
    prefix: String,
    csrf_token: String,
}

/// A POST to the /mark form, which is displayed on the create page. This
/// creates a new dogear, then displays the marked page (non-slowmode).
#[tracing::instrument(skip(state, auth))]
pub async fn post_mark(
    State(state): State<DogState>,
    auth: AuthSession,
    Form(params): Form<CreateParams>,
) -> WebResult<Html<String>> {
    if params.csrf_token != auth.session.csrf_token {
        return Err(WebError::new(
            StatusCode::BAD_REQUEST,
            r#"The create-new-dogear form was stale or mangled.
                Go back, refresh that page, and try marking your spot again."#
                .to_string(),
        ));
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

/// Given a URL, do one of the following:
/// - If there's an existing dogear, redirect straight to the currently marked page for it.
/// - If not, render the create page.
/// - If logged out, show the login page.
/// Since this might be a Redirect OR a page, we can't return `impl IntoResponse`; gotta
/// manually convert first and return Response.
#[tracing::instrument(skip_all)]
pub async fn resume(
    State(state): State<DogState>,
    maybe_auth: Option<AuthSession>,
    Path(url): Path<String>,
    own_uri: Uri,
    cookies: Cookies,
) -> WebResult<Response> {
    let Some(auth) = maybe_auth else {
        let path = own_uri.to_string();
        return Ok(login_form(state, cookies, &path).await?.into_response());
    };
    match state
        .db
        .dogears()
        .current_for_site(auth.user.id, &url)
        .await?
    {
        Some(current) => Ok(Redirect::to(&current).into_response()),
        None => {
            let create_page = CreatePage {
                bookmarked_url: &url,
            };
            let common = auth.common_args("Dogear this?");
            let ctx = context! {create_page, common};
            Ok(Html(state.render_view("create.html.j2", ctx)?).into_response())
        }
    }
}

/// Display the faq/news/about page. This is almost a static page, but
/// if there's a user around, we want them for the layout header.
#[tracing::instrument(skip_all)]
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

#[tracing::instrument(skip_all)]
pub async fn post_fragment_personalmark(
    State(state): State<DogState>,
    auth: AuthSession,
    Query(params): Query<PersonalMarkParams>,
) -> WebResult<(StatusCode, Html<String>)> {
    if params.csrf_token != auth.session.csrf_token {
        return Err(WebError::new(
            StatusCode::BAD_REQUEST,
            r#"The bookmarklet generate button was stale or mangled.
                Refresh the page and try generating again."#
                .to_string(),
        ));
    }
    // Skip an alloc w/ format_into:
    let mut comment_bytes: Vec<u8> = "Personal bookmarklet created ".into();
    OffsetDateTime::now_utc()
        .format_into(&mut comment_bytes, SHORT_DATE)
        .map_err(|_| UserError::Impossible("time format_into vec failed"))?;
    let comment = String::from_utf8(comment_bytes)
        .map_err(|_| UserError::Impossible("statically known utf8 wasn't utf8"))?;
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

#[tracing::instrument(skip_all)]
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
#[tracing::instrument(skip_all)]
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
#[tracing::instrument(skip_all)]
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
#[tracing::instrument(skip_all)]
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
#[tracing::instrument(skip_all)]
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
pub struct DeleteAccountParams {
    password: String,
    confirm_delete_account: String,
    csrf_token: String,
}

/// The delete account form, on the account page. It's kind of like the Final Logout.
#[tracing::instrument(skip_all)]
pub async fn post_delete_account(
    State(state): State<DogState>,
    auth: AuthSession,
    cookies: Cookies,
    Form(params): Form<DeleteAccountParams>,
) -> WebResult<Redirect> {
    if params.csrf_token != auth.session.csrf_token {
        return Err(WebError::new(
            StatusCode::BAD_REQUEST,
            r#"The delete account form you tried to use was stale, or
                had been tampered with. Go back to the account page and try
                deleting your account again."#
                .to_string(),
        ));
    }
    let users = state.db.users();
    // authenticate the password, validate the confirm string, waste the
    // session cookie, delete the user (which will cascade to all foreign key refs).
    let Some(user) = users
        .authenticate(&auth.user.username, &params.password)
        .await?
    else {
        return Err(WebError::new(
            StatusCode::BAD_REQUEST,
            "Wrong password".to_string(),
        ));
    };
    if params.confirm_delete_account.trim() != DELETE_ACCOUNT_CONFIRM_STRING {
        return Err(WebError::new(
            StatusCode::BAD_REQUEST,
            format!(
                r#"Wrong delete confirmation string; you must type the exact phrase
                "delete my account" (without the quotation marks) into the account delete
                form, but you typed "{}" instead. Go back to the account page and try
                deleting your account again."#,
                &params.confirm_delete_account
            ),
        ));
    }
    // OK, at this point we're ready to party.
    // The From impl on WebError uses ToString, so throwing a str is fine.
    users.destroy(user.id).await?.ok_or(UserError::Impossible(
        "User not found! That shouldn't be possible at this point??",
    ))?;
    cookies.remove(auth.session.as_ref().clone().into_cookie());

    Ok(Redirect::to("/"))
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
#[tracing::instrument(skip_all)]
pub async fn post_login(
    State(state): State<DogState>,
    cookies: Cookies,
    req_headers: HeaderMap,
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
        .public_url
        .join(&params.return_to)
        .unwrap_or_else(|_| state.config.public_url.clone());
    if redirect_to.origin() != state.config.public_url.origin() {
        redirect_to = state.config.public_url.clone();
    }

    // then, authenticate user and tack on a session cookie.
    if let Some(user) = state
        .db
        .users()
        .authenticate(&params.username, &params.password)
        .await?
    {
        let user_agent = req_headers
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok());
        let session = state.db.sessions().create(user.id, user_agent).await?;
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
    // Really email'll always be present (and maybe blank), but downstream
    // recipients expect an Option and will flatmap it to normalize.
    email: Option<String>,
    login_csrf_token: String,
}

/// Handle POSTs from the signup form. This always appears alongside the login form.
/// It has some of the same properties, but it always redirects to /.
#[tracing::instrument(skip_all)]
pub async fn post_signup(
    State(state): State<DogState>,
    cookies: Cookies,
    req_headers: HeaderMap,
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
        return Err(WebError::new(
            StatusCode::FORBIDDEN,
            "Can't sign up while you're still logged in. (How'd you even do that, by the way?)"
                .to_string(),
        ));
    }
    if let Err(e) = check_new_password(&params.new_password, &params.new_password_again) {
        return Err(WebError::new(StatusCode::BAD_REQUEST, e.to_string()));
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
    let user_agent = req_headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok());
    let session = state.db.sessions().create(user.id, user_agent).await?;
    cookies.add(session.into_cookie());
    Ok(Redirect::to("/"))
}

#[derive(Deserialize, Debug)]
pub struct ChangeEmailParams {
    password: String,
    // Always present, but gonna flat-map and pass directly to set_email.
    new_email: Option<String>,
    csrf_token: String,
}

/// The change email form, on the account page.
#[tracing::instrument(skip_all)]
pub async fn post_change_email(
    State(state): State<DogState>,
    auth: AuthSession,
    Form(params): Form<ChangeEmailParams>,
) -> WebResult<Redirect> {
    if params.csrf_token != auth.session.csrf_token {
        return Err(WebError::new(
            StatusCode::BAD_REQUEST,
            r#"The change email form you tried to use was stale, or
                had been tampered with. Go back to the account page and try
                changing your email again."#
                .to_string(),
        ));
    }
    let users = state.db.users();
    let Some(user) = users
        .authenticate(&auth.user.username, &params.password)
        .await?
    else {
        return Err(WebError::new(
            StatusCode::BAD_REQUEST,
            "Wrong password".to_string(),
        ));
    };
    let new_email = clean_optional_form_field(params.new_email.as_deref());
    users.set_email(&user.username, new_email).await?;
    Ok(Redirect::to("/account?changed=email"))
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
#[tracing::instrument(skip_all)]
pub async fn post_changepassword(
    State(state): State<DogState>,
    auth: AuthSession,
    Form(params): Form<ChangePasswordParams>,
) -> WebResult<Redirect> {
    if params.csrf_token != auth.session.csrf_token {
        return Err(WebError::new(
            StatusCode::BAD_REQUEST,
            r#"The change password form you tried to use was stale, or
                had been tampered with. Go back to the account page and try
                changing your password again."#
                .to_string(),
        ));
    }
    if let Err(e) = check_new_password(&params.new_password, &params.new_password_again) {
        return Err(WebError::new(StatusCode::BAD_REQUEST, e.to_string()));
    }
    let users = state.db.users();
    let Some(user) = users
        .authenticate(&auth.user.username, &params.password)
        .await?
    else {
        return Err(WebError::new(
            StatusCode::BAD_REQUEST,
            "Wrong existing password".to_string(),
        ));
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
#[tracing::instrument(skip(state, cookies))]
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

#[derive(Serialize, Deserialize, Debug)]
pub struct ApiMeta {
    pub pagination: Pagination,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ApiDogearsList {
    pub data: Vec<Dogear>,
    pub meta: ApiMeta,
}

impl ApiDogearsList {
    fn new(dogears: Vec<Dogear>, list_meta: ListMeta) -> Self {
        Self {
            data: dogears,
            meta: ApiMeta {
                pagination: list_meta.to_pagination(),
            },
        }
    }
}

#[tracing::instrument(skip_all)]
pub async fn api_list(
    State(state): State<DogState>,
    auth: AuthAny,
    Query(params): Query<PaginationQuery>,
) -> ApiResult<Json<ApiDogearsList>> {
    // Requires manage
    auth.allowed_scopes(&[TokenScope::ManageDogears])?;
    let (dogears, meta) = state
        .db
        .dogears()
        .list(auth.user().id, params.page(), params.size())
        .await?;
    Ok(Json(ApiDogearsList::new(dogears, meta)))
}

#[tracing::instrument(skip_all)]
pub async fn api_delete(
    State(state): State<DogState>,
    auth: AuthAny,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    // Requires manage
    auth.allowed_scopes(&[TokenScope::ManageDogears])?;
    if state
        .db
        .dogears()
        .destroy(id, auth.user().id)
        .await?
        .is_some()
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "dogear not found".to_string(),
        ))
    }
}

#[derive(Deserialize, Debug)]
pub struct ApiCreatePayload {
    prefix: String,
    current: String,
    display_name: Option<String>,
}

#[tracing::instrument(skip(state, auth))]
pub async fn api_create(
    State(state): State<DogState>,
    auth: AuthAny,
    Json(payload): Json<ApiCreatePayload>,
) -> ApiResult<(StatusCode, Json<Dogear>)> {
    // Both manage and write are ok
    auth.allowed_scopes(&[TokenScope::WriteDogears, TokenScope::ManageDogears])?;
    let res = state
        .db
        .dogears()
        .create(
            auth.user().id,
            &payload.prefix,
            &payload.current,
            payload.display_name.as_deref(),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(res)))
}

// Mutates a HeaderMap in-place to set the necessary CORS headers for a given
// origin. This is hardcoded for the needs of the /api/v1/update endpoint,
// because it's literally the only thing we do that needs cors, so it's not
// worth investing in tower-http's CorsLayer yet.
fn set_cors_headers_for_api_update(headers: &mut HeaderMap, origin: &str) -> Result<(), UserError> {
    headers.insert(header::VARY, HeaderValue::from_name(header::ORIGIN));
    // First off, we no longer do cookie auth on CORS, it's tokens or the highway. So:
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
        HeaderValue::from_static("false"),
    );
    // Whoever you are: u are valid. (Actually, I think we could get away
    // with *, now that we're not accepting credentials. Nevertheless!)
    // Don't mind the map_err; this basically can't happen if the http::Request
    // made it this far.
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        origin.parse().map_err(|_| UserError::HttpFucked)?,
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("POST"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("Content-Type, Authorization, Content-Length, X-Requested-With"),
    );
    Ok(())
}

#[tracing::instrument(skip_all)]
pub async fn api_update_cors_preflight(
    State(state): State<DogState>,
    req_headers: HeaderMap,
) -> ApiResult<(StatusCode, HeaderMap)> {
    // At this point, there might or might not be an API token or session auth in
    // play. We're not really gonna care until we reach the actual POST, tho.
    // Just answer _as though_ they were properly auth'd.
    let mut res_headers = HeaderMap::new();

    if let Some(origin) = req_headers.get(header::ORIGIN) {
        if let Ok(origin) = origin.to_str() {
            if origin != state.config.public_url.origin().ascii_serialization() {
                // Then it's a CORS-eligible cross-origin request! Tack on them headers.
                set_cors_headers_for_api_update(&mut res_headers, origin)?
            }
        }
    }

    Ok((StatusCode::NO_CONTENT, res_headers))
}

#[derive(Deserialize, Debug)]
pub struct ApiUpdatePayload {
    current: String,
}

#[tracing::instrument(skip_all)]
pub async fn api_update(
    State(state): State<DogState>,
    req_headers: HeaderMap,
    auth: AuthAny,
    Json(payload): Json<ApiUpdatePayload>,
) -> ApiResult<(HeaderMap, Json<Vec<Dogear>>)> {
    // Both write and manage tokens are ok here.
    auth.allowed_scopes(&[TokenScope::WriteDogears, TokenScope::ManageDogears])?;

    let mut res_headers = HeaderMap::new();

    if let Some(origin) = req_headers.get(header::ORIGIN) {
        if let Ok(origin) = origin.to_str() {
            if origin != state.config.public_url.origin().ascii_serialization() {
                // Then it's a CORS-eligible cross-origin request!

                // OK, first, CORS ACCESS CHECK.
                // Requests from other sites may only update your bookmark on THAT site.
                let Ok(to_bookmark) = Url::parse(&payload.current) else {
                    return Err(UserError::DogearInvalidUrl {
                        url: payload.current,
                    }
                    .into());
                };
                if to_bookmark.origin().ascii_serialization() != origin {
                    return Err(UserError::Dogear404.into());
                }

                // Ok, looks like we're good to go. Tack on them headers.
                set_cors_headers_for_api_update(&mut res_headers, origin)?
            }
        }
    }

    match state
        .db
        .dogears()
        .update(auth.user().id, &payload.current)
        .await?
    {
        Some(ds) => Ok((res_headers, Json(ds))),
        None => Err(UserError::Dogear404.into()),
    }
}
