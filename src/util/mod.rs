use anyhow::anyhow;
use rand::{thread_rng, RngCore};
use sha2::{Digest, Sha256};

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
