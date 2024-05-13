use super::core::Db;
use crate::util::{clean_optional_form_field, MixedError, UserError};

use lazy_static::lazy_static;
use regex::Regex;
use serde::Serialize;
use sqlx::{error::ErrorKind, query, query_as, SqlitePool};
use time::OffsetDateTime;
use tracing::error;

/// A query helper type for operating on [User]s. Usually you rent this from
/// a [Db].
#[derive(Debug)]
pub struct Users<'a> {
    db: &'a Db,
}

/// Record struct for user accounts.
#[derive(Debug, PartialEq, Clone, Serialize)]
pub struct User {
    pub id: i64, // Unfortunately,
    pub username: String,
    pub email: Option<String>,
    pub created: OffsetDateTime,
}

// Private struct for type-checked queries
struct UserWithPasswordHash {
    id: i64,
    username: String,
    email: Option<String>,
    created: OffsetDateTime,
    password_hash: String,
}

impl From<UserWithPasswordHash> for User {
    fn from(v: UserWithPasswordHash) -> Self {
        Self {
            id: v.id,
            username: v.username,
            email: v.email,
            created: v.created,
        }
    }
}

// Some helpers!

/// Trim whitespace and validate allowed username characters.
/// Ascii letters/numbers/joiners is too restrictive, but now's not the
/// time to loosen it. Maybe later.
fn clean_username(username: &str) -> Result<&str, UserError> {
    lazy_static! {
        static ref USERNAME_REGEX: Regex = Regex::new(r#"\A[a-zA-Z0-9_-]{1,80}\z"#).unwrap();
    }
    let username = username.trim();
    if USERNAME_REGEX.is_match(username) {
        Ok(username)
    } else {
        Err(UserError::BadUsername {
            name: username.to_string(),
        })
    }
}
fn valid_password(password: &str) -> Result<&str, UserError> {
    if password.is_empty() {
        Err(UserError::BlankPassword)
    } else {
        Ok(password)
    }
}

// create, authenticate, set_password, change_password, set_email, destroy
impl<'a> Users<'a> {
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }
    fn read_pool(&self) -> &SqlitePool {
        &self.db.read_pool
    }
    fn write_pool(&self) -> &SqlitePool {
        &self.db.write_pool
    }

    /// Create a new user account.
    #[tracing::instrument(skip_all)]
    pub async fn create(
        &self,
        username: &str,
        password: &str,
        email: Option<&str>,
    ) -> Result<User, MixedError<sqlx::Error>> {
        let username = clean_username(username)?;
        let email = clean_optional_form_field(email);
        let password = valid_password(password)?;
        let password_hash = bcrypt::hash(password, 12).map_err(|_| {
            UserError::Impossible("bcrypt hash of statically-known cost had illegal cost")
        })?;

        query_as!(
            User,
            r#"
                INSERT INTO users (username, password_hash, email)
                VALUES (?1, ?2, ?3)
                RETURNING id, username, email, created;
            "#,
            username,
            password_hash,
            email,
        )
        .fetch_one(self.write_pool())
        .await
        .map_err(|e| match e {
            // Need to catch unique constraint violation and return friendly error; any
            // other sqlx errors are 500s in this case.
            sqlx::Error::Database(dbe) if dbe.kind() == ErrorKind::UniqueViolation => {
                UserError::UserExists {
                    name: username.to_string(),
                }
                .into()
            }
            _ => e.into(),
        })
    }

    /// Fetch a user and their password hash, by name. Deliberately not public API.
    #[tracing::instrument(skip_all)]
    async fn by_name_with_password_hash(
        &self,
        username: &str,
    ) -> sqlx::Result<Option<UserWithPasswordHash>> {
        let username = username.trim();

        query_as!(
            UserWithPasswordHash,
            r#"
                SELECT id, username, email, created, password_hash
                FROM users WHERE username = ?;
            "#,
            username
        )
        .fetch_optional(self.read_pool()) // NICE!!!!
        .await
    }

    /// Test helper: Just fetch a user. App logic should always find users
    /// via the `authenticate` methods on Users / Sessions / Tokens.
    #[cfg(test)]
    pub async fn by_name(&self, username: &str) -> sqlx::Result<Option<User>> {
        Ok(self
            .by_name_with_password_hash(username)
            .await?
            .map(|u| u.into()))
    }

    /// Authenticate a user by username and password. Only returns Some if the
    /// user exists and the password matches.
    #[tracing::instrument(skip_all)]
    pub async fn authenticate(
        &self,
        username: &str,
        password: &str,
    ) -> anyhow::Result<Option<User>> {
        if let Some(user) = self.by_name_with_password_hash(username).await? {
            // Reason this function has to return an anyhow is bc there's
            // several unlikely reasons bcrypt::verify can fail and they're
            // all worthy of 500 errors.
            if bcrypt::verify(password, &user.password_hash)? {
                return Ok(Some(user.into()));
            }
        }
        Ok(None)
    }

    /// Hard-set a user's password. IMPORTANT: assumes you've already validated the inputs!
    #[tracing::instrument(skip_all)]
    pub async fn set_password(
        &self,
        username: &str,
        new_password: &str,
    ) -> Result<(), MixedError<sqlx::Error>> {
        let password_hash = bcrypt::hash(new_password, 12).map_err(|_| {
            UserError::Impossible("bcrypt hash of statically-known cost had illegal cost")
        })?;

        let res = query!(
            r#"
                UPDATE users SET password_hash = ?1
                WHERE username = ?2;
            "#,
            password_hash,
            username,
        )
        .execute(self.write_pool())
        .await?;
        if res.rows_affected() != 1 {
            error!(%username, "unable to find logged-in user");
            Err(UserError::Impossible("user is both logged-in and nonexistent").into())
        } else {
            Ok(())
        }
    }

    /// Set or clear the user's email. BTW, this and set_password take username
    /// instead of ID in order to give better errors, since these errors
    /// will definitely flow all the way up to the frontend.
    #[tracing::instrument(skip_all)]
    pub async fn set_email(
        &self,
        username: &str,
        email: Option<&str>,
    ) -> Result<(), MixedError<sqlx::Error>> {
        let email = clean_optional_form_field(email);

        let res = query!(
            r#"
                UPDATE users SET email = ?1
                WHERE username = ?2;
            "#,
            email,
            username,
        )
        .execute(self.write_pool())
        .await?;
        if res.rows_affected() != 1 {
            error!(%username, "unable to find logged-in user");
            Err(UserError::Impossible("user is both logged-in and nonexistent").into())
        } else {
            Ok(())
        }
    }

    /// Returns Ok(Some) on success, Ok(None) on not-found.
    #[tracing::instrument(skip_all)]
    pub async fn destroy(&self, id: i64) -> sqlx::Result<Option<()>> {
        let res = query!(
            r#"
                DELETE FROM users WHERE id = ?;
            "#,
            id,
        )
        .execute(self.write_pool())
        .await?;
        if res.rows_affected() == 1 {
            Ok(Some(()))
        } else {
            Ok(None)
        }
    }
}
