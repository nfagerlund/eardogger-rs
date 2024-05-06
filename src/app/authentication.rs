//! tl;dr:
//!
//! - Add both middlewares to the app, making sure the token one runs after
//!   the session one. Wholly-static routes with no user variance (/public, 404...)
//!   can go outside the auth middlewares.
//! - AuthSession is a subset of AuthAny.
//! - Most "web page" routes should use the AuthSession extractor to get a user.
//! - API routes can use the AuthAny extractor, and should immediately call
//!   `.allowed_scopes()?` on the value.

use super::state::DogState;
use super::web_result::{ApiError, AppError, AppErrorKind};
use crate::db::{Session, Token, TokenScope, User};
use crate::util::COOKIE_SESSION;
use axum::{
    async_trait,
    extract::{FromRequestParts, Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};
use http::{header, request::Parts, HeaderMap, HeaderValue, StatusCode};
use std::fmt::Debug;
use std::sync::Arc;
use tower_cookies::Cookies;

// ok let's get our types in a row.
// The db types all use String for text because that's what Sqlx demands,
// but practically speaking they're immutable once they come out the db.
// So while we can't save money on the initial allocation, we CAN at least
// say "never again" at this point, once we check out some medium-lifespan
// working copies. Theoretically this saves a couple pointless clones
// on every web page route that demands an AuthSession.

/// The main authenticated user type within the app. This is what gets
/// stored in a request extension by the auth middleware, and it's also
/// available as an extractor.
#[derive(Clone, Debug)]
pub enum AuthAny {
    Session {
        user: Arc<User>,
        session: Arc<Session>,
    },
    Token {
        user: Arc<User>,
        token: Arc<Token>,
    },
}

impl AuthAny {
    /// A lil helper for throwing early-out 403 errors with the `?` operator,
    /// in routes that only allow specific token scopes. We assume that a "real"
    /// login session has a _superset_ of all possible token permissions for
    /// that user, so session auth is always allowed through. This always assumes
    /// it's returning a JSON error, because it only errors if you _actually_
    /// provided a valid token, which means you're well on your way to doing an
    /// api request.
    pub fn allowed_scopes(&self, scopes: &[TokenScope]) -> Result<(), ApiError> {
        match self {
            AuthAny::Session { .. } => Ok(()),
            AuthAny::Token { token, .. } => {
                if scopes.iter().any(|s| *s == token.scope()) {
                    Ok(())
                } else {
                    Err(ApiError::new(
                        StatusCode::FORBIDDEN,
                        "The provided authentication token doesn't have the right permissions to perform this action.".to_string()),
                    )
                }
            }
        }
    }

    /// A lil helper for getting quick access to the authenticated user without
    /// having to do a match.
    pub fn user(&self) -> Arc<User> {
        match self {
            AuthAny::Session { user, .. } => user.clone(),
            AuthAny::Token { user, .. } => user.clone(),
        }
    }
}

/// Only available as an extractor, the info in here is sourced from an
/// AuthAny::Session value if one exists in the request extensions.
#[derive(Clone, Debug)]
pub struct AuthSession {
    pub user: Arc<User>,
    pub session: Arc<Session>,
}

impl AuthSession {
    /// A little helper to build common template args, give that most of it
    /// is loaned out of the auth session anyway.
    pub fn common_args<'a>(&'a self, title: &'a str) -> super::templates::Common<'a> {
        super::templates::Common {
            title,
            user: Some(&*self.user),
            csrf_token: &self.session.csrf_token,
        }
    }
}

// Checks both the Accept and Content-Type (in case of POST/PUT) headers to
// see if we should be returning json error objects; defaults to html otherwise.
fn error_kind_from_headers(headers: &HeaderMap<HeaderValue>) -> AppErrorKind {
    if let Some(v) = headers.get(http::header::ACCEPT) {
        if header_val_matches(v, "application/json") {
            return AppErrorKind::Json;
        }
    }
    if let Some(v) = headers.get(http::header::CONTENT_TYPE) {
        if header_val_matches(v, "application/json") {
            return AppErrorKind::Json;
        }
    }
    AppErrorKind::Html
}

// True if the header value is a valid string AND equals the provided text.
fn header_val_matches(val: &HeaderValue, text: &str) -> bool {
    match val.to_str() {
        Ok(matchable) => matchable == text,
        Err(_) => false,
    }
}

// These extractors rely on the session and token middlewares being present in
// the stack. If they're not around, extraction always whiffs.
//
// Routes should pick their extractor based on which kinds of auth they allow.
// Most routes only want a login user and should use AuthSession, but API routes
// that accept token auth should use AuthAny and then call
// `auth.allowed_scopes(&[...])?` to validate the token scope before continuing.

#[async_trait]
impl<S> FromRequestParts<S> for AuthAny
where
    S: Send + Sync + Debug,
{
    type Rejection = AppError;

    #[tracing::instrument(skip_all)]
    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // We default to HTML error pages... but if this is specifically a json request,
        // we'll remember to render json errors later.
        let kind = error_kind_from_headers(&parts.headers);

        match parts.extensions.get::<AuthAny>() {
            Some(aa) => Ok(aa.clone()),
            None => Err(AppError::new(
                StatusCode::UNAUTHORIZED,
                "Either you aren't logged in, you forgot to pass a token, or your token is no longer valid.".to_string(),
                kind,
            )),
        }
    }
}

#[async_trait]
impl<S> FromRequestParts<S> for AuthSession
where
    S: Send + Sync + Debug,
{
    type Rejection = AppError;

    #[tracing::instrument(skip_all)]
    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // We default to HTML error pages... but if this is specifically a json request,
        // we'll remember to render json errors later.
        let kind = error_kind_from_headers(&parts.headers);

        if let Some(AuthAny::Session { user, session }) = parts.extensions.get::<AuthAny>() {
            Ok(AuthSession {
                user: user.clone(),
                session: session.clone(),
            })
        } else {
            Err(AppError::new(
                StatusCode::UNAUTHORIZED,
                "You aren't logged in, so you can't do this. Go back and reload the page to start over.".to_string(),
                kind,
            ))
        }
    }
}

// So, about those middlewares... how's about a refresher.
//
// My auth middleware is deeply entangled with the way I store and authenticate
// users, so there would be no point in making it redistributable. It's
// the definition of bespoke!
//
// The simplest way to make non-redistributable middleware is with
// from_fn_with_state(). You feed it an async fn that takes extractors
// as arguments, the last extractor arg MUST consume the body, and the LAST
// last arg is a Next fn.
//
// Both these middlewares are applied to almost every route, and routes can
// use the extractors to pick which kinds of auth they accept. The token fn must
// run AFTER the session fn, and will blow away the session user if a token
// was actually provided.

/// Function middleware to validate a login session and make the logged-in user
/// available to routes.
#[tracing::instrument(skip_all)]
pub async fn session_middleware(
    State(state): State<DogState>,
    cookies: Cookies,
    mut request: Request,
    next: Next,
) -> Response {
    let error_kind = error_kind_from_headers(request.headers());

    // get sessid out of cookie
    if let Some(sessid) = cookies.get(COOKIE_SESSION) {
        match state.db.sessions().authenticate(sessid.value()).await {
            Ok(maybe) => {
                if let Some((session, user)) = maybe {
                    // ok rad, do it
                    request.extensions_mut().insert(AuthAny::Session {
                        user: Arc::new(user),
                        session: Arc::new(session.clone()),
                    });
                    // Update cookie with new expiration date...
                    // tower_cookies will ship this on the outbound leg.
                    cookies.add(session.into_cookie());
                }
            }
            Err(e) => {
                // If this hit a DB error, the site can't do much, so feel free to bail.
                return AppError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string(), error_kind)
                    .into_response();
            }
        }
    }
    // if we made it here, it's time to move on!
    next.run(request).await
}

/// Function middleware to validate a token passed in the `Authorization: Bearer STUFF`
/// header and make the token's user available to routes. This overrides the session
/// user if both would have been present.
#[tracing::instrument(skip_all)]
pub async fn token_middleware(
    State(state): State<DogState>,
    mut request: Request,
    next: Next,
) -> Response {
    // Actually this'll pretty much always be json if you're providing a token...
    // but hey, no harm in checking.
    let error_kind = error_kind_from_headers(request.headers());

    if let Some(auth_header) = request.headers().get(header::AUTHORIZATION) {
        if let Ok(auth_val) = auth_header.to_str() {
            if let Some(bearer_val) = auth_val.strip_prefix("Bearer ") {
                // phew!!
                let token_cleartext = bearer_val.trim();
                match state.db.tokens().authenticate(token_cleartext).await {
                    Ok(maybe) => {
                        if let Some((token, user)) = maybe {
                            // ok rad, do it! This will blow away the session user, if any.
                            // (Token inclusion is a stronger intent than cookie presence.)
                            request.extensions_mut().insert(AuthAny::Token {
                                user: Arc::new(user),
                                token: Arc::new(token),
                            });
                        }
                    }
                    Err(e) => {
                        // If this hit a DB error, the site can't do much, so feel free to bail.
                        return AppError::new(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            e.to_string(),
                            error_kind,
                        )
                        .into_response();
                    }
                }
            }
        }
    }
    // Ok, carry on
    next.run(request).await
}
