mod bookmarklets;
mod url_encoding;

use anyhow::anyhow;
use rand::{thread_rng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use time::{format_description::FormatItem, macros::format_description};
use url::Url;

pub use bookmarklets::*;
pub use url_encoding::*;

// Constants
/// A time crate format description, like this: 2024-3-22
pub const SHORT_DATE: &[FormatItem] =
    format_description!("[year]-[month repr:numerical padding:none]-[day padding:none]");
/// The session cookie name. This is a pre-existing value from eardogger 1...
/// not that those sessions will be valid anymore, but re-using it should help
/// reduce junk cookie pollution. 👍🏼
pub const COOKIE_SESSION: &str = "eardogger.sessid";
/// The login form signed anti-CSRF cookie name. Most "plain" forms use
/// an anti-CSRF token stored in the session, but the session doesn't exist
/// until after you log in, so.
pub const COOKIE_LOGIN_CSRF: &str = "eardogger.loginguard";
pub const PAGE_DEFAULT_SIZE: u32 = 50;
const PAGE_MAX_SIZE: u32 = 500;

/// Use the thread_rng CSPRNG to create a random UUID, formatted as a String.
/// This ought to be mildly more efficient than hammering the OS random source.
/// Not that we especially care, probably!
pub fn uuid_string() -> String {
    let mut bytes = [0u8; 16];
    thread_rng().fill_bytes(&mut bytes);
    let uu = uuid::Builder::from_random_bytes(bytes).into_uuid();
    uu.as_hyphenated().to_string()
}

/// Calculate the sha256 checksum of a &str and return it as a lowercase hex
/// String. There's several competing ways to make this more efficient, but I'm
/// not currently going to do them:
/// - Store the checksums as raw blobs -- but, I have existing data where they're
///   text, so nah.
/// - Fill a &str that points at a fixed-length stack-allocated buffer, instead
///   of allocating a String. Maybe later, for fun.
pub fn sha256sum(cleartext: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(cleartext);
    let hash = hasher.finalize();
    base16ct::lower::encode_string(&hash)
}

/// Metadata about which fraction of a collection was returned by a
/// list method, for building pagination affordances.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ListMeta {
    pub count: u32,
    pub page: u32,
    pub size: u32,
}

impl ListMeta {
    pub fn to_pagination(self) -> Pagination {
        let total_pages = self.count.div_ceil(self.size);
        // page 0 isn't a thing:
        let current_page = self.page.max(1);
        let page_size = if self.size == PAGE_DEFAULT_SIZE {
            None
        } else {
            Some(self.size)
        };
        let prev_page = if current_page == 1 {
            None
        } else {
            // Guardrail if you hacked the query param and paged past the end.
            Some((current_page - 1).min(total_pages))
        };
        let next_page = if current_page >= total_pages {
            None
        } else {
            Some(current_page + 1)
        };
        Pagination {
            current_page,
            page_size,
            prev_page,
            next_page,
            total_pages,
            total_count: self.count,
        }
    }
}

/// Pagination details built from a ListMeta, useful when displaying
/// page-turning controls in a template.
#[derive(Serialize, Deserialize, Debug)]
pub struct Pagination {
    pub current_page: u32,
    // The reason page_size is optional is so that you get cleaner URLs if
    // you didn't override the default size.
    pub page_size: Option<u32>,
    pub prev_page: Option<u32>,
    pub next_page: Option<u32>,
    pub total_pages: u32,
    pub total_count: u32,
}

/// Given a (1-indexed) page and size, calculate an OFFSET value to pass
/// to a sqlite query. Sqlite integers in sqlx are pretty much always i64,
/// so this is messier than it feels like it wants to be.
pub fn sqlite_offset(page: u32, size: u32) -> anyhow::Result<i64> {
    let zero_idx_page = page
        .checked_sub(1)
        .ok_or_else(|| anyhow!("Invalid page number."))?;
    if size > PAGE_MAX_SIZE {
        return Err(anyhow!("Requested page size is too large."));
    }
    let size_i64: i64 = size.into();
    let zero_idx_page_i64: i64 = zero_idx_page.into();

    // This also can't fail, with MAX_PAGE_SIZE set to 500.
    size_i64.checked_mul(zero_idx_page_i64).ok_or_else(|| {
        anyhow!("Literally impossible, but apparently page * size overflowed an i64.")
    })
}

#[derive(Error, Debug)]
pub enum NewPasswordError {
    #[error("New passwords didn't match.")]
    NonMatching,
    #[error("New password can't be empty.")]
    Empty,
}

pub fn check_new_password(new1: &str, new2: &str) -> Result<(), NewPasswordError> {
    if new1 != new2 {
        Err(NewPasswordError::NonMatching)
    } else if new1.is_empty() {
        Err(NewPasswordError::Empty)
    } else {
        Ok(())
    }
}

/// Axum's `Form` fields show up as `Some("")` if they're present but empty,
/// but we have a few functions that want to be able to omit empty fields.
/// So the convention is that they're marked as `Option<String>` in the relevant
/// form params struct, and the downstream function that receives it can use this
/// lil flatmapper to clean its inputs.
pub fn clean_optional_form_field(maybe: Option<&str>) -> Option<&str> {
    maybe.and_then(|e| {
        let e = e.trim();
        if e.is_empty() {
            None
        } else {
            Some(e)
        }
    })
}

/// Trim any leading "m." or "www." subdomains off a hostname at the start
/// of a string. (Generally you'll call this function with *most* of a URL,
/// after first removing the scheme and the `://` separator.)
fn trim_m_www(mut partial_url: &str) -> &str {
    loop {
        if partial_url.starts_with("m.") {
            partial_url = &partial_url["m.".len()..];
        } else if partial_url.starts_with("www.") {
            partial_url = &partial_url["www.".len()..];
        } else {
            break;
        }
    }
    partial_url
}

/// Validate that the input is an HTTP or HTTPS URL, then remove the scheme and
/// the `://` separator. The result can be passed to [`trim_m_www`].
fn trim_and_check_scheme(url: &str) -> anyhow::Result<&str> {
    let Ok(parsed) = Url::parse(url) else {
        return Err(anyhow!("Can't bookmark an invalid URL: {}", url));
    };
    let scheme = parsed.scheme();
    if scheme == "http" || scheme == "https" {
        let trim_len = scheme.len() + "://".len();
        let sliced = &url[trim_len..];
        Ok(sliced)
    } else {
        Err(anyhow!(
            "Only http or https URLs are supported; we can't bookmark {}",
            scheme
        ))
    }
}

/// Turn a given URL into a partial URL (path and hostname with
/// any `m.` or `www.` subdomains trimmed) that can be comparied to a
/// stored prefix string with a simple `matchable LIKE prefix || '%'`
/// SQL expression (or a `.starts_with()` if you're in normal code).
/// This also doubles as a check for valid input URLs.
pub fn matchable_from_url(url: &str) -> anyhow::Result<&str> {
    Ok(trim_m_www(trim_and_check_scheme(url)?))
}

/// Clean and normalize a provided prefix matcher string before persisting it.
/// A cleaned prefix can reliably match the results of `matchable_from_url`.
pub fn normalize_prefix_matcher(prefix: &str) -> &str {
    // The input shouldn't have a URL scheme, so we normally expect to
    // just eat this error. But if we *happen* to have an http(s) scheme,
    // go ahead and trim it, since the user's intent was still clear.
    let scheme_trimmed = match trim_and_check_scheme(prefix) {
        Ok(s) => s,
        Err(_) => prefix,
    };
    trim_m_www(scheme_trimmed)
}

#[cfg(test)]
mod tests {
    use crate::util::{normalize_prefix_matcher, trim_m_www};

    use super::trim_and_check_scheme;

    #[test]
    fn m_and_www() {
        assert_eq!(trim_m_www("m.example.com"), "example.com");
        assert_eq!(trim_m_www("www.example.com"), "example.com");
        assert_eq!(trim_m_www("m.www.example.com"), "example.com");
        assert_eq!(trim_m_www("www.m.example.com"), "example.com");
        assert_eq!(trim_m_www("somewhere.example.com"), "somewhere.example.com");
    }

    #[test]
    fn scheme_trim() {
        assert_eq!(
            trim_and_check_scheme("https://example.com/comic").unwrap(),
            "example.com/comic"
        );
        assert_eq!(
            trim_and_check_scheme("http://example.com/comic").unwrap(),
            "example.com/comic"
        );
        assert!(trim_and_check_scheme("noscheme.example.com/comic").is_err());
        assert!(trim_and_check_scheme("ftp://example.com/comic.tgz").is_err());
    }

    #[test]
    fn matcher_normalizing() {
        assert_eq!(normalize_prefix_matcher("m.example.com"), "example.com");
        assert_eq!(normalize_prefix_matcher("www.example.com"), "example.com");
        assert_eq!(normalize_prefix_matcher("m.www.example.com"), "example.com");
        assert_eq!(normalize_prefix_matcher("www.m.example.com"), "example.com");
        assert_eq!(
            normalize_prefix_matcher("somewhere.example.com"),
            "somewhere.example.com"
        );
        assert_eq!(
            normalize_prefix_matcher("http://www.m.example.com"),
            "example.com"
        );
        // If you do this one, you just fucked up and need to fix it, we can't help ya:
        assert_eq!(
            normalize_prefix_matcher("ftp://www.m.example.com"),
            "ftp://www.m.example.com"
        );
    }

    use super::clean_optional_form_field;

    #[test]
    fn clean_optional_test() {
        assert_eq!(clean_optional_form_field(None), None);
        assert_eq!(
            clean_optional_form_field(Some("you@example.com")),
            Some("you@example.com")
        );
        assert_eq!(
            clean_optional_form_field(Some("me@example.com ")),
            Some("me@example.com")
        );
        assert_eq!(clean_optional_form_field(Some("")), None);
    }
}
