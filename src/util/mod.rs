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
