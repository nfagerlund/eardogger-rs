use super::dogears::Dogears;
use super::sessions::Sessions;
use super::tokens::{TokenScope, Tokens};
use super::users::Users;
use sqlx::SqlitePool;

/// The app's main database helper type. One of these goes in the app state,
/// and you can use it to access all the various resource methods, namespaced
/// for readability.
#[derive(Clone, Debug)]
pub struct Db {
    pub read_pool: SqlitePool,
    pub write_pool: SqlitePool,
}

impl Db {
    /// yeah.
    pub fn new(pool: SqlitePool) -> Self {
        // TEMP, TODO: take write_pool in contructor args
        let write_pool = pool.clone();
        Self {
            read_pool: pool,
            write_pool,
        }
    }

    pub fn users(&self) -> Users {
        Users::new(self)
    }

    pub fn tokens(&self) -> Tokens {
        Tokens::new(self)
    }

    pub fn dogears(&self) -> Dogears {
        Dogears::new(self)
    }

    pub fn sessions(&self) -> Sessions {
        Sessions::new(self)
    }
}

// Test stuff, kept a lil separate from the main stuff.
impl Db {
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
}

/// A helper struct for setting up data in tests.
#[allow(dead_code)]
pub struct TestUser {
    pub name: String,
    pub write_token: String,
    pub manage_token: String,
    pub session_id: String,
}
