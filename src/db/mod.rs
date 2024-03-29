//! Ok, how about this. We make a Db type that wraps a pool, and a bunch
//! of pluralized collection types (Users, etc.) that just hold a reference
//! to a pool, and methods on Db that return those collections. So then it's
//! like `db.users().create(...)`. Seems ok.
mod db_tests;
mod dogears;
mod sessions;
mod tokens;
mod users;
use self::dogears::Dogears;
use self::sessions::Sessions;
use self::tokens::Tokens;
use self::users::Users;
use sqlx::SqlitePool;

// Publicize the record types, they're the star of the show
pub use self::dogears::Dogear;
pub use self::sessions::Session;
pub use self::tokens::{Token, TokenScope};
pub use self::users::User;

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

    pub async fn new_test_db() -> Self {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("sqlx-ploded during migrations");
        Self::new(pool)
    }

    pub fn users(&self) -> Users {
        Users::new(&self.pool)
    }

    pub fn tokens(&self) -> Tokens {
        Tokens::new(&self.pool)
    }

    pub fn dogears(&self) -> Dogears {
        Dogears::new(&self.pool)
    }

    pub fn sessions(&self) -> Sessions {
        Sessions::new(&self.pool)
    }
}
