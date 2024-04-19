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
#[derive(Clone, Debug)]
pub struct Db {
    pool: SqlitePool,
}

/// A helper struct for setting up data in tests.
#[allow(dead_code)]
pub struct TestUser {
    pub name: String,
    pub write_token: String,
    pub manage_token: String,
    pub session_id: String,
}

impl Db {
    /// yeah.
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    // this is for tests, of course it's dead in real builds.
    #[allow(dead_code)]
    pub async fn new_test_db() -> Self {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("sqlx-ploded during migrations");
        Self::new(pool)
    }

    /// Test helper. Create a new user with:
    /// - Provided name
    /// - Password "aoeuhtns"
    /// - A write token and a manage token
    /// - An active login session
    /// - Two bookmarks
    #[allow(dead_code)]
    pub async fn test_user(&self, name: &str) -> anyhow::Result<TestUser> {
        let (users, tokens, sessions, dogears) =
            (self.users(), self.tokens(), self.sessions(), self.dogears());
        let email = format!("{}@example.com", name);

        let user = users.create(name, "aoeuhtns", Some(&email)).await?;
        let (_, write_token) = tokens
            .create(
                user.id,
                TokenScope::WriteDogears,
                Some("write token for test user"),
            )
            .await?;
        let (_, manage_token) = tokens
            .create(
                user.id,
                TokenScope::ManageDogears,
                Some("manage token for test user"),
            )
            .await?;
        let session = sessions.create(user.id).await?;
        dogears
            .create(
                user.id,
                "example.com/comic",
                "https://example.com/comic/24",
                Some("Example Comic"),
            )
            .await?;
        dogears
            .create(
                user.id,
                "example.com/serial",
                "https://example.com/serial/4",
                Some("Example Serial"),
            )
            .await?;

        Ok(TestUser {
            name: user.username,
            write_token,
            manage_token,
            session_id: session.id,
        })
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
