use super::core::Db;
use sqlx::{
    migrate::{Migrate, Migrator},
    query_scalar, SqlitePool,
};
use std::{borrow::Cow, collections::HashMap, fmt::Display};
use thiserror::Error;
use tracing::{debug, warn};

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

/// The current state of an individual database migration.
#[derive(Debug)]
pub enum Status {
    Applied {
        version: i64,
        description: Cow<'static, str>,
    },
    Busted {
        version: i64,
        description: Cow<'static, str>,
        applied_checksum: Cow<'static, [u8]>,
        intended_checksum: Cow<'static, [u8]>,
    },
    Pending {
        version: i64,
        description: Cow<'static, str>,
    },
    Unrecognized {
        version: i64,
        description: String,
    },
}

impl Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Status::Applied {
                version: v,
                description: d,
            } => {
                write!(f, "{v} (applied)    {d}")
            }
            Status::Busted {
                version: v,
                description: d,
                applied_checksum: have,
                intended_checksum: want,
            } => {
                // Checksum is a sha384 as raw bytes. Format the first 8 bytes as
                // 16 hex characters for short display.
                write!(f, "{v} (!!BUSTED!!) {d} - have ")?;
                for b in &have[0..8] {
                    write!(f, "{b:02x?}")?;
                }
                write!(f, ", want ")?;
                for b in &want[0..8] {
                    write!(f, "{b:02x?}")?;
                }
                Ok(())
            }
            Status::Pending {
                version: v,
                description: d,
            } => {
                write!(f, "{v} (pending)    {d}")
            }
            Status::Unrecognized {
                version: v,
                description: d,
            } => {
                write!(f, "{v} (!!FUTURE!!) {d}")
            }
        }
    }
}

impl<'a> Migrations<'a> {
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }

    fn write_pool(&self) -> &SqlitePool {
        &self.db.write_pool
    }

    /// Run any pending migrations on the database.
    #[tracing::instrument(skip_all)]
    pub async fn run(&self) -> Result<(), sqlx::migrate::MigrateError> {
        MIGRATOR.run(self.write_pool()).await
    }

    /// Check whether the database migrations are in a usable state. For background
    /// on the logic in here, consult the source of the sqlx CLI:
    /// https://github.com/launchbadge/sqlx/blob/5d6c33ed65cc2/sqlx-cli/src/migrate.rs
    /// We're doing a fast and dirty version of the same thing.
    #[tracing::instrument(skip_all)]
    pub async fn validate(&self) -> anyhow::Result<()> {
        // Using write pool bc there's a small chance of CREATE TABLE.
        // Also this happens before normal operation so we aren't worried about contention.
        let mut conn = self.write_pool().acquire().await?;
        conn.ensure_migrations_table().await?;
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
            match applied_migrations.remove(&known.version) {
                Some(checksum) => {
                    if checksum != known.checksum {
                        errs.wrong_checksum += 1;
                    }
                }
                None => errs.unapplied += 1,
            }
        }
        unrecognized += applied_migrations.len();
        debug!("{} known migrations", total_known);
        if unrecognized > 0 {
            warn!(
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

    /// Basically a wordier version of .validate(), meant for printing info to the terminal.
    #[tracing::instrument(skip_all)]
    pub async fn info(&self) -> anyhow::Result<Vec<Status>> {
        // Using write pool bc there's a small chance of CREATE TABLE.
        // Also this happens before normal operation so we aren't worried about contention.
        let mut conn = self.write_pool().acquire().await?;
        conn.ensure_migrations_table().await?;
        let mut applied_migrations: HashMap<_, _> = conn
            .list_applied_migrations()
            .await?
            .into_iter()
            .map(|m| (m.version, m.checksum))
            .collect();

        // This pass catches all the known migrations, then we handle unknowns afterwards.
        let mut statuses: Vec<Status> = MIGRATOR
            .iter()
            .filter(|&m| !m.migration_type.is_down_migration())
            .map(|known| match applied_migrations.remove(&known.version) {
                Some(checksum) => {
                    if checksum == known.checksum {
                        Status::Applied {
                            version: known.version,
                            description: known.description.clone(),
                        }
                    } else {
                        Status::Busted {
                            version: known.version,
                            description: known.description.clone(),
                            applied_checksum: checksum,
                            intended_checksum: known.checksum.clone(),
                        }
                    }
                }
                None => Status::Pending {
                    version: known.version,
                    description: known.description.clone(),
                },
            })
            .collect();

        // OK, anything left is unknown.
        if !applied_migrations.is_empty() {
            let mut unknowns: Vec<i64> = applied_migrations.keys().cloned().collect();
            unknowns.sort();
            for version in unknowns.into_iter() {
                let description = query_scalar!(
                    r#"SELECT description FROM _sqlx_migrations WHERE version = ?;"#,
                    version
                )
                .fetch_one(&mut *conn)
                .await?;
                statuses.push(Status::Unrecognized {
                    version,
                    description,
                });
            }
        }

        Ok(statuses)
    }
}
