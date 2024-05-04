use std::collections::HashMap;

use super::dogears::Dogears;
use super::sessions::Sessions;
use super::tokens::Tokens;
use super::users::Users;
use sqlx::{
    migrate::{Migrate, Migrator},
    SqlitePool,
};
use thiserror::Error;
use tokio_util::task::TaskTracker;
use tracing::{debug, info};

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

// A baked-in stacic copy of all the database migrations.
static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

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

    pub async fn run_migrations(&self) -> Result<(), sqlx::migrate::MigrateError> {
        MIGRATOR.run(&self.write_pool).await
    }

    /// Check whether the database migrations are in a usable state. For background
    /// on the logic in here, consult the source of the sqlx CLI:
    /// https://github.com/launchbadge/sqlx/blob/5d6c33ed65cc2/sqlx-cli/src/migrate.rs
    /// We're doing basically the same thing.
    pub async fn validate_migrations(&self) -> anyhow::Result<()> {
        let mut conn = self.read_pool.acquire().await?;
        let mut applied_migrations: HashMap<_, _> = conn
            .list_applied_migrations()
            .await?
            .into_iter()
            .map(|m| (m.version, m.checksum))
            .collect();

        let mut errs = MigrationError::default();
        let mut unrecognized = 0usize;
        let mut total_known = 0usize;

        for known in MIGRATOR
            .iter()
            .filter(|&m| !m.migration_type.is_down_migration())
        {
            total_known += 1;
            match applied_migrations.get(&known.version) {
                Some(checksum) => {
                    if *checksum != known.checksum {
                        errs.wrong_checksum += 1;
                    }
                }
                None => errs.unapplied += 1,
            }
            applied_migrations.remove(&known.version);
        }
        unrecognized += applied_migrations.len();
        debug!("{} known migrations", total_known);
        if unrecognized > 0 {
            info!(
                "{} unrecognized database migrations; are you running an old app version?",
                unrecognized
            );
        }

        if errs.any() {
            Err(errs.into())
        } else {
            Ok(())
        }
    }
}

#[derive(Error, Default, Debug)]
#[error("bad migration situation: {unapplied} unapplied, {wrong_checksum} busted.")]
pub struct MigrationError {
    wrong_checksum: usize,
    unapplied: usize,
}

impl MigrationError {
    pub fn any(&self) -> bool {
        self.wrong_checksum + self.unapplied > 0
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
        MIGRATOR
            .run(&write_pool)
            .await
            .expect("sqlx-ploded during migrations");
        let read_pool = write_pool.clone();
        Self::new(read_pool, write_pool, TaskTracker::new())
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
