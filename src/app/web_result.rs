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
    // Build an html-fragment description of the error, to be included
    // in an error page later.
    fn from(value: E) -> Self {
        let mut message = String::new();
        // TODO: Probably would like to suppress detailed errors for prod.
        message.push_str("<p>");
        html_escape::encode_safe_to_string(value.to_string(), &mut message);
        message.push_str("</p>");

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
