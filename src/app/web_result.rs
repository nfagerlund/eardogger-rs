//! Right, so the story here goes something like this.
//!
//! - A bunch of my app-level logic is fallible, like trying to access
//!   something that doesn't belong to your user or is 404.
//! - Also, a bunch of library logic is fallible, like database access
//!   or template fetch/render calls.
//! - A route will do several of these things, so it'd be nice to use `?` on
//!   some of the intermediate ones.
//!
//! Therefore, routes'll need to return a Result type, and the Error type
//! will need to implement IntoResponse... AND, the Error type will ALSO
//! need to implement `From<T>` for any intermediate error type I'll be
//! invoking `?` on.
//!
//! Once I've got that, the next problem is that I can't use a template
//! for my error page... because template fetch+render is fallible, and
//! this is the last line of defense! So, time for a duplicated partial
//! page skeleton and a `format!()` call.
//!
//! The concept here is adapted from this blog post:
//! <https://fasterthanli.me/series/updating-fasterthanli-me-for-2022/part-2>
//! I don't need a bunch of the extra complexity from that, tho. He's
//! using backtraces and stuff and is requiring a detour through the
//! eyre::Report error type, so he can't just do a blanket impl for
//! T: Error.

use crate::config::is_production;
use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

/// An IntoResponse-implementing type that can display error content as either
/// an HTML error page, or a JSON error object. By using wrapper types that
/// implement appropriate From traits, handlers can use the `?` operator to
/// smoothly handle fallible operations like DB access or template rendering,
/// and reserve any wordier error handling for the specific app logic they own.
#[derive(Debug)]
pub struct AppError {
    pub message: String,
    pub status: StatusCode,
    pub kind: AppErrorKind,
}

#[derive(Debug)]
pub enum AppErrorKind {
    Html,
    Json,
}

// A dumb Serialize wrapper for `{ "error":"blah blah" }` so I don't have to
// use the dynamic json!() object macro.
#[derive(Serialize, Deserialize, Debug)]
pub struct RawJsonError {
    pub error: Cow<'static, str>,
}

impl AppError {
    pub fn new(status: StatusCode, message: String, kind: AppErrorKind) -> Self {
        Self {
            status,
            message,
            kind,
        }
    }
}

impl IntoResponse for AppError {
    // Depending on our error kind, return either an error html page or an error json object.
    #[tracing::instrument]
    fn into_response(self) -> Response {
        let Self {
            message,
            status,
            kind,
        } = self;
        // Suppress 500 error details for prod. (Other error codes are fine,
        // but 500s could be pretty much anything.)
        let message = if is_production() && status == StatusCode::INTERNAL_SERVER_ERROR {
            Cow::from(
                r#"The server had a problem and couldn't recover. This is
                probably a bug in the site."#,
            )
        } else {
            Cow::from(message)
        };

        match kind {
            AppErrorKind::Html => {
                let mut text = String::new();
                text.push_str("<p>");
                html_escape::encode_safe_to_string(&message, &mut text);
                text.push_str("</p>");

                let page = format!(include_str!("../../templates/_error.html"), &text);
                (status, Html(page)).into_response()
            }
            AppErrorKind::Json => {
                let body = RawJsonError { error: message };
                (status, Json(body)).into_response()
            }
        }
    }
}

// Now for the wrapper types! Each of these must implement:
// - From<E> where E has some trait bound to sweep up all the errors we
//   want to bubble. Unfortunately there's some awkwardness due to using
//   anyhow::Error in some places, so right now that bound is ToString.
// - IntoResponse (by just delegating to the inner AppError).

/// An AppError wrapper type for handlers that return HTML web pages.
#[derive(Debug)]
pub struct WebError(pub AppError);

/// A convenience type for returning probably an Ok(IntoResponse), or maybe
/// an error page, from a route.
pub type WebResult<T> = Result<T, WebError>;

impl WebError {
    pub fn new(status: StatusCode, message: String) -> Self {
        Self(AppError::new(status, message, AppErrorKind::Html))
    }
}

// Well, I wanted to dive into the potential nested errors from Error::source(),
// but it turns out that anyhow::Error _does not implement std::error::Error!_
// Hoisted by my own whatever you call it!!! The more I think about it, the more
// it makes sense that that would be the case, it just never occurred to me when
// I was laying out the database helpers. Anyway, because a::E doesn't implement
// Error but _might_ theoretically in the future, rust won't let me have both a
// blanket impl for E: Error and a specific impl for anyhow::Error, so oof.
// I think the workaround for now is, you don't get to see the source errors,
// and I'll just use a trait bound that fits both std error and anyhow error.
// I could reconsider that in the future by following what fasterthanlime did and
// enforcing a side-trip through anyhow for all errors, or I could just deal.
impl<E: ToString> From<E> for WebError {
    fn from(value: E) -> Self {
        // For quick-and-dirty error returns, use a default HTTP error code of 500.
        // This is almost always correct.
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, value.to_string())
    }
}

impl IntoResponse for WebError {
    fn into_response(self) -> Response {
        self.0.into_response()
    }
}

/// An AppError wrapper type for routes that return JSON objects.
#[derive(Debug)]
pub struct ApiError(pub AppError);

/// A convenience type for returning probably an Ok(IntoResponse), or maybe
/// an error object, from a route.
pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    pub fn new(status: StatusCode, message: String) -> Self {
        Self(AppError::new(status, message, AppErrorKind::Json))
    }
}

impl<E: ToString> From<E> for ApiError {
    fn from(value: E) -> Self {
        // For quick-and-dirty error returns, use a default HTTP error code of 500.
        // This is almost always correct.
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, value.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        self.0.into_response()
    }
}
