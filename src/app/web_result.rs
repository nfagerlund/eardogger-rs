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
//! need to implement From<T> for any intermediate error type I'll be
//! invoking `?` on.
//!
//! Once I've got that, the next problem is that I can't use a template
//! for my error page... because template fetch+render is fallible, and
//! this is the last line of defense! So, time for a duplicated partial
//! page skeleton and a `format!()` call.
//!
//! The concept here is adapted from this blog post:
//! https://fasterthanli.me/series/updating-fasterthanli-me-for-2022/part-2
//! I don't need a bunch of the extra complexity from that, tho. He's
//! using backtraces and stuff and is requiring a detour through the
//! eyre::Report error type, so he can't just do a blanket impl for
//! T: Error.

use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
use std::error::Error;

/// An IntoResponse type that any error can be converted to, for displaying
/// HTML error pages from a route.
pub struct WebError {
    pub message: String,
    pub status: StatusCode,
}

/// A convenience type for returning probably an Ok(IntoResponse), or maybe
/// an error page, from a route.
pub type WebResult<T> = Result<T, WebError>;

impl WebError {
    pub fn new(status: StatusCode, message: String) -> Self {
        Self { message, status }
    }
}

impl<E: Error> From<E> for WebError {
    // Build an html-fragment description of the error, to be included
    // in an error page later.
    fn from(value: E) -> Self {
        let mut message = String::new();
        // if the error happens to have nested source errors, list em all.
        // TODO: Probably would like to suppress detailed errors for prod.
        let mut err: &dyn Error = &value;
        loop {
            message.push_str("<p>");
            html_escape::encode_safe_to_string(err.to_string(), &mut message);
            message.push_str("</p>");
            if let Some(next) = err.source() {
                err = next;
            } else {
                break;
            }
        }

        // For quick-and-dirty error returns, use a default HTTP error code of 500.
        // This is almost always correct.
        Self {
            message,
            status: StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for WebError {
    fn into_response(self) -> Response {
        let page = format!(include_str!("../../templates/_error.html"), &self.message);
        (self.status, Html(page)).into_response()
    }
}
