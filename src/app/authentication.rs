use crate::db::{Db, Session, Token, User};
use axum::{
    async_trait,
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
};
use std::sync::Arc;

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
enum AuthAny {
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
struct AuthSession {
    user: Arc<User>,
    session: Arc<Session>,
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

// So, about those middlewares...
