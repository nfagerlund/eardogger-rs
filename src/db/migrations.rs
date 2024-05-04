use super::core::Db;
use sqlx::{
    migrate::{Migrate, Migrator},
    SqlitePool,
};
use std::collections::HashMap;
use thiserror::Error;
use tracing::{debug, info};

// A baked-in stacic copy of all the database migrations.
static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// A db helper for running and inspecting migrations. Usually you only
/// want to touch this during startup or in an alternative run mode,
/// not while actually serving requests.
#[derive(Debug)]
pub struct Migrations<'a> {
    db: &'a Db,
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

impl<'a> Migrations<'a> {
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }

    fn read_pool(&self) -> &SqlitePool {
        &self.db.read_pool
    }
    fn write_pool(&self) -> &SqlitePool {
        &self.db.write_pool
    }

    /// Run any pending migrations on the database.
    pub async fn run(&self) -> Result<(), sqlx::migrate::MigrateError> {
        MIGRATOR.run(self.write_pool()).await
    }

    /// Check whether the database migrations are in a usable state. For background
    /// on the logic in here, consult the source of the sqlx CLI:
    /// https://github.com/launchbadge/sqlx/blob/5d6c33ed65cc2/sqlx-cli/src/migrate.rs
    /// We're doing basically the same thing.
    pub async fn validate(&self) -> anyhow::Result<()> {
        let mut conn = self.read_pool().acquire().await?;
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
