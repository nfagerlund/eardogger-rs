//! OK, so here's the scoop: we have some javascript files. We want to turn
//! them into bookmarklets (i.e. minify then mangle with url-encoding). But
//! we want to *maintain* them in unmangled form.
//!
//! Also, one of the bookmarklets requires a token to be inserted at runtime,
//! and they both need a URL inserted at runtime. So they're templates, too,
//! and they have different escaping rules than HTML templates.
//!
//! Ideally, we don't want to pay the cost of mangling these things at request
//! time, or even at startup time (since startup time is pretty frequent under
//! my desired deployment conditions). And it occurred to me that I could use
//! a build script to pre-minify and url-encode the templates, then make sure
//! to insert double-escaped (JS string context + url context) text at runtime.
//! However, turns out this is a slightly harder problem than just performing
//! existing eardogger 1 logic at a different point in time, because of the
//! `{{ template expressions }}` -- they're gonna get whomped by the url-encoding
//! if I try to pre-load that, so I'd need to write some logic to "url-encode
//! EXCEPT for template expressions and/or tags", and, buddy, whoof.
//!
//! So anyway, long story short we're doing this with a lil bit of wasteful
//! computation instead of my beatiful excessively optimized concept. But,
//! maybe someday.

use super::url_encoding::encode_uri_component;
use lazy_static::lazy_static;
use regex::Regex;

lazy_static! {
    static ref COMMENTED_LINES: Regex = Regex::new(r#"^\s*//.*\n"#).unwrap();
    static ref LEADING_SPACE: Regex = Regex::new(r#"^\s+"#).unwrap();
    static ref LINE_ENDINGS: Regex = Regex::new(r#"\s*\n"#).unwrap();
    static ref SPACE_RUNS: Regex = Regex::new(r#"\s{2,}"#).unwrap();
}

/// Convert a valid javascript ...script, into a minified `javascript:` URL,
/// i.e. a bookmarklet.
/// This minifier is naïve and not especially safe! It expects the caller
/// to uphold the following invariants:
/// - No block comments (`/* ... */`), only line comments (`//`).
/// - Line comments must always be on their own line, no commenting the end of
///   a line.
/// - Semicolons are mandatory.
pub fn make_bookmarklet(js: &str) -> String {
    let wip = COMMENTED_LINES.replace_all(js, "");
    let wip = LEADING_SPACE.replace_all(&wip, "");
    let wip = LINE_ENDINGS.replace_all(&wip, "");
    let wip = SPACE_RUNS.replace_all(&wip, " ");
    format!("javascript:{}", encode_uri_component(&wip))
}
