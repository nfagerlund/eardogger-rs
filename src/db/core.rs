use super::dogears::Dogears;
use super::migrations::Migrations;
use super::sessions::Sessions;
use super::tokens::Tokens;
use super::users::Users;
use sqlx::SqlitePool;
use tokio_util::task::TaskTracker;

/// The app's main database helper type. One of these goes in the app state,
/// and you can use it to access all the various resource methods, namespaced
/// for readability.
#[derive(Clone, Debug)]
pub struct Db {
    pub read_pool: SqlitePool,
    pub write_pool: SqlitePool,
    // Query helpers may spawn SHORT-LIVED async tasks, so need a tracker but not a cancel token.
    pub task_tracker: TaskTracker,
}

impl Db {
    /// yeah.
    pub fn new(read_pool: SqlitePool, write_pool: SqlitePool, task_tracker: TaskTracker) -> Self {
        Self {
            read_pool,
            write_pool,
            task_tracker,
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

    pub fn migrations(&self) -> Migrations {
        Migrations::new(self)
    }
}

// Test stuff, kept a lil separate from the main stuff.
impl Db {
    #[cfg(test)]
    pub async fn new_test_db() -> Self {
        use sqlx::Sqlite;
        use sqlx::{
            pool::PoolOptions,
            sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous},
        };
        use std::str::FromStr;
        use std::time::Duration;

        // Match the connect options from normal operation...
        let db_opts = SqliteConnectOptions::from_str("sqlite::memory:").unwrap();
        let db_opts = db_opts
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5))
            .pragma("temp_store", "memory")
            .optimize_on_close(true, 400)
            .synchronous(SqliteSynchronous::Normal) // usually fine w/ wal
            .foreign_keys(true);
        // ...but cap the connections to 1 so we can just serialize everything.
        let pool_opts: PoolOptions<Sqlite> = PoolOptions::new()
            .max_connections(1) // default's 10, but we'll be explicit.
            .min_connections(1);

        let write_pool = pool_opts.connect_with(db_opts).await.unwrap();
        let read_pool = write_pool.clone();
        let db = Self::new(read_pool, write_pool, TaskTracker::new());
        db.migrations()
            .run()
            .await
            .expect("sqlx-ploded during migrations");
        db
    }

    /// Wait for any async writes to settle before continuing to test any
    /// related conditions. This shouldn't ever be used in real operation,
    /// because the task tracker will contain tasks that won't end until
    /// shutdown time, but it's potentially useful in tests.
    #[cfg(test)]
    pub async fn test_flush_tasks(&self) {
        self.task_tracker.close();
        self.task_tracker.wait().await;
        self.task_tracker.reopen();
    }

    /// Test helper. Create a new user with:
    /// - Provided name
    /// - Password "aoeuhtns"
    /// - A write token and a manage token
    /// - An active login session
    /// - Two bookmarks
    #[cfg(test)]
    pub async fn test_user(&self, name: &str) -> anyhow::Result<TestUser> {
        use super::tokens::TokenScope;

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
#[cfg(test)]
pub struct TestUser {
    pub name: String,
    pub write_token: String,
    pub manage_token: String,
    pub session_id: String,
}
