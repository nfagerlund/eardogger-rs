//! Ok, how about this. We make a Db type that wraps a pool, and a bunch
//! of pluralized collection types (Users, etc.) that just hold a reference
//! to a pool, and methods on Db that return those collections. So then it's
//! like `db.users().create(...)`. Seems ok.
mod db_tests;
mod dogears;
mod sessions;
mod tokens;
mod users;
use crate::util::{PasswordHasher, RealPasswordHasher, WorstPasswordHasher};
use dogears::Dogears;
use sessions::Sessions;
use sqlx::SqlitePool;
use tokens::Tokens;
use users::Users;

/// The app's main database helper type. One of these goes in the app state,
/// and you can use it to access all the various resource methods, namespaced
/// for readability.
#[derive(Clone)]
pub struct Db<H> {
    pool: SqlitePool,
    password_hasher: H,
}

impl Db<RealPasswordHasher> {
    /// yeah.
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            password_hasher: RealPasswordHasher,
        }
    }
}

impl Db<WorstPasswordHasher> {
    pub async fn new_test_db() -> Self {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("sqlx-ploded during migrations");
        Self {
            pool,
            password_hasher: WorstPasswordHasher,
        }
    }
}

impl<H> Db<H>
where
    H: PasswordHasher + Clone,
{
    pub fn users(&self) -> Users<H> {
        Users::new(&self.pool, self.password_hasher.clone())
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
