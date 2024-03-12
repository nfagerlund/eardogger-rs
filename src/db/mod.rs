//! Ok, how about this. We make a Db type that wraps a pool, and a bunch
//! of pluralized collection types (Users, etc.) that just hold a reference
//! to a pool, and methods on Db that return those collections. So then it's
//! like `db.users().create(...)`. Seems ok.
mod tokens;
mod users;
use sqlx::SqlitePool;

/// The app's main database helper type. One of these goes in the app state,
/// and you can use it to access all the various resource methods, namespaced
/// for readability.
#[derive(Clone)]
pub struct Db {
    pool: SqlitePool,
}

impl Db {
    /// yeah.
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}
