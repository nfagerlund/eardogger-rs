use super::state::DogState;
use crate::db::{Db, Session, Token, User};
use crate::util::COOKIE_SESSION;
use axum::{
    async_trait,
    extract::{FromRequestParts, Request, State},
    http::{header, request::Parts, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
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

// These extractors rely on the session and token middlewares being present in
// the stack. If they're not around, it always whiffs.
#[async_trait]
impl<S> FromRequestParts<S> for AuthAny
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match parts.extensions.get::<AuthAny>() {
            Some(aa) => Ok(aa.clone()),
            None => Err((StatusCode::UNAUTHORIZED, "Either you aren't logged in, you forgot to pass a token, or your token is no longer valid.")),
        }
    }
}

#[async_trait]
impl<S> FromRequestParts<S> for AuthSession
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        if let Some(AuthAny::Session { user, session }) = parts.extensions.get::<AuthAny>() {
            Ok(AuthSession {
                user: user.clone(),
                session: session.clone(),
            })
        } else {
            Err((StatusCode::UNAUTHORIZED, "You aren't logged in, so you can't do this. Go back and reload the page to start over."))
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
// In my case, the session middleware will be applied to every route, but the
// token one will only be applied to API routes. The token middleware expects
// to run AFTER the session one, and will blow away the session user if a token
// was actually provided.

/// Function middleware to validate a login session and make the logged-in user
/// available to routes.
pub async fn session_middleware(
    State(state): State<DogState>,
    cookies: Cookies,
    mut request: Request,
    next: Next,
) -> Response {
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
                return db_error_response_tuple(e, state.config.is_prod).into_response();
            }
        }
    }
    // if we made it here, it's time to move on!
    next.run(request).await
}

/// Function middleware to validate a token passed in the `Authorization: Bearer STUFF`
/// header and make the token's user available to routes. This should only be applied
/// to API routes, and it overrides the session user if both would have been present.
pub async fn token_middleware(
    State(state): State<DogState>,
    mut request: Request,
    next: Next,
) -> Response {
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
                        return db_error_response_tuple(e, state.config.is_prod).into_response();
                    }
                }
            }
        }
    }
    // Ok, carry on
    next.run(request).await
}

/// Small helper to obscure DB error text from users in production.
fn db_error_response_tuple(e: anyhow::Error, is_prod: bool) -> (StatusCode, String) {
    let msg = if is_prod {
        "Something's broken on the server, sorry!".to_string()
    } else {
        format!("DB error: {}", e)
    };
    (StatusCode::INTERNAL_SERVER_ERROR, msg)
}
