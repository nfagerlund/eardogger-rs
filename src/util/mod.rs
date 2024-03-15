use anyhow::anyhow;
use rand::{thread_rng, RngCore};
use sha2::{Digest, Sha256};
use url::Url;

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

/// Mini trait for ways to hash and verify user login passwords.
/// If I had a public API for this I'd probably want to make the error
/// type generic, but I'm gonna just anyhow it.
pub trait PasswordHasher {
    fn hash(&self, password: &str) -> anyhow::Result<String>;
    fn verify(&self, password: &str, hash: &str) -> anyhow::Result<bool>;
}

/// The normal hasher.
#[derive(Clone, Copy, Debug)]
pub struct RealPasswordHasher;
impl PasswordHasher for RealPasswordHasher {
    fn hash(&self, password: &str) -> anyhow::Result<String> {
        bcrypt::hash(password, 12).map_err(|e| e.into())
    }

    fn verify(&self, password: &str, hash: &str) -> anyhow::Result<bool> {
        bcrypt::verify(password, hash).map_err(|e| e.into())
    }
}

/// NEVER USE THIS IN REAL CODE -- it only exists to speed up the tests.
#[derive(Clone, Copy, Debug)]
pub struct WorstPasswordHasher;
impl PasswordHasher for WorstPasswordHasher {
    fn hash(&self, password: &str) -> anyhow::Result<String> {
        Ok(sha256sum(password))
    }

    fn verify(&self, password: &str, hash: &str) -> anyhow::Result<bool> {
        if sha256sum(password) == hash {
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

/// Metadata about which fraction of a collection was returned by a
/// list method, for building pagination affordances.
#[derive(Clone, Copy)]
pub struct ListMeta {
    pub count: u32,
    pub page: u32,
    pub size: u32,
}

const DEFAULT_PAGE_SIZE: u32 = 50;
const MAX_PAGE_SIZE: u32 = 500;

/// Given a (1-indexed) page and size, calculate an OFFSET value to pass
/// to a sqlite query. Sqlite integers in sqlx are pretty much always i64,
/// so this is messier than it feels like it wants to be.
pub fn sqlite_offset(page: u32, size: u32) -> anyhow::Result<i64> {
    let zero_idx_page = page
        .checked_sub(1)
        .ok_or_else(|| anyhow!("Invalid page number."))?;
    if size > MAX_PAGE_SIZE {
        return Err(anyhow!("Requested page size is too large."));
    }
    // These can't fail.
    let size_i64: i64 = size.try_into()?;
    let zero_idx_page_i64: i64 = zero_idx_page.try_into()?;

    // This also can't fail, with MAX_PAGE_SIZE set to 500.
    size_i64.checked_mul(zero_idx_page_i64).ok_or_else(|| {
        anyhow!("Literally impossible, but apparently page * size overflowed an i64.")
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
fn check_and_trim_scheme(url: &str) -> anyhow::Result<&str> {
    let Ok(parsed) = Url::parse(url) else {
        return Err(anyhow!("Can't bookmark an invalid URL: {}", url));
    };
    let scheme = parsed.scheme();
    if scheme == "http" || scheme == "https" {
        let sliced = &url[scheme.len()..];
        let sliced = &sliced["://".len()..];
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
    Ok(trim_m_www(check_and_trim_scheme(url)?))
}

/// Clean and normalize a provided prefix matcher string before persisting it.
/// A cleaned prefix can reliably match the results of `matchable_from_url`.
pub fn normalize_prefix_matcher(prefix: &str) -> &str {
    // The input shouldn't have a URL scheme, so we normally expect to
    // just eat this error. But if we *happen* to have an http(s) scheme,
    // go ahead and trim it, since the user's intent was still clear.
    let scheme_trimmed = match check_and_trim_scheme(prefix) {
        Ok(s) => s,
        Err(_) => prefix,
    };
    trim_m_www(scheme_trimmed)
}

#[cfg(test)]
mod tests {
    use crate::util::{normalize_prefix_matcher, trim_m_www};

    use super::check_and_trim_scheme;

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
            check_and_trim_scheme("https://example.com/comic").unwrap(),
            "example.com/comic"
        );
        assert_eq!(
            check_and_trim_scheme("http://example.com/comic").unwrap(),
            "example.com/comic"
        );
        assert!(check_and_trim_scheme("noscheme.example.com/comic").is_err());
        assert!(check_and_trim_scheme("ftp://example.com/comic.tgz").is_err());
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
}
