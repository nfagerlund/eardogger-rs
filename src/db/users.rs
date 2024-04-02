use crate::util::clean_optional_form_field;

use anyhow::anyhow;
use lazy_static::lazy_static;
use regex::Regex;
use serde::Serialize;
use sqlx::{query, query_as, SqlitePool};
use time::OffsetDateTime;

/// A query helper type for operating on [User]s. Usually you rent this from
/// a [Db].
#[derive(Debug)]
pub struct Users<'a> {
    pool: &'a SqlitePool,
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
fn clean_username(username: &str) -> anyhow::Result<&str> {
    lazy_static! {
        static ref USERNAME_REGEX: Regex = Regex::new(r#"\A[a-zA-Z0-9_-]{1,80}\z"#).unwrap();
    }
    let username = username.trim();
    if USERNAME_REGEX.is_match(username) {
        Ok(username)
    } else {
        Err(anyhow!(
            r#"Can't use "{}" as a username on this site. Usernames can only use letters, numbers, hyphens (-), and underscores (_), and can't be longer than 80 characters."#,
            username
        ))
    }
}
fn valid_password(password: &str) -> anyhow::Result<&str> {
    if password.is_empty() {
        Err(anyhow!("Empty password isn't allowed."))
    } else {
        Ok(password)
    }
}

// create, authenticate, set_password, change_password, set_email, destroy
impl<'a> Users<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Create a new user account.
    #[tracing::instrument]
    pub async fn create(
        &self,
        username: &str,
        password: &str,
        email: Option<&str>,
    ) -> anyhow::Result<User> {
        let username = clean_username(username)?;
        let email = clean_optional_form_field(email);
        let password = valid_password(password)?;
        let password_hash = bcrypt::hash(password, 12)?;

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
        .fetch_one(self.pool)
        .await
        .map_err(|e| e.into())
    }

    /// Fetch a user and their password hash, by name. Deliberately not public API.
    #[tracing::instrument]
    async fn by_name_with_password_hash(
        &self,
        username: &str,
    ) -> anyhow::Result<Option<UserWithPasswordHash>> {
        let username = username.trim();

        query_as!(
            UserWithPasswordHash,
            r#"
                SELECT id, username, email, created, password_hash
                FROM users WHERE username = ?;
            "#,
            username
        )
        .fetch_optional(self.pool) // NICE!!!!
        .await
        .map_err(|e| e.into())
    }

    /// Just fetch a user. Most app logic should use [`authenticate`] instead,
    /// but this is nice to have in tests.
    pub async fn by_name(&self, username: &str) -> anyhow::Result<Option<User>> {
        Ok(self
            .by_name_with_password_hash(username)
            .await?
            .map(|u| u.into()))
    }

    /// Authenticate a user by username and password. Only returns Some if the
    /// user exists and the password matches.
    #[tracing::instrument]
    pub async fn authenticate(
        &self,
        username: &str,
        password: &str,
    ) -> anyhow::Result<Option<User>> {
        if let Some(user) = self.by_name_with_password_hash(username).await? {
            if bcrypt::verify(password, &user.password_hash)? {
                return Ok(Some(user.into()));
            }
        }
        Ok(None)
    }

    /// Hard-set a user's password. IMPORTANT: assumes you've already validated the inputs!
    #[tracing::instrument]
    pub async fn set_password(&self, username: &str, new_password: &str) -> anyhow::Result<()> {
        let password_hash = bcrypt::hash(new_password, 12)?;
        let res = query!(
            r#"
                UPDATE users SET password_hash = ?1
                WHERE username = ?2;
            "#,
            password_hash,
            username,
        )
        .execute(self.pool)
        .await?;
        if res.rows_affected() != 1 {
            Err(anyhow!("Couldn't find user with name {}.", username))
        } else {
            Ok(())
        }
    }

    /// Set or clear the user's email. BTW, this and set_password take username
    /// instead of ID in order to give better errors, since these errors
    /// will definitely flow all the way up to the frontend.
    pub async fn set_email(&self, username: &str, email: Option<&str>) -> anyhow::Result<()> {
        let email = clean_optional_form_field(email);

        let res = query!(
            r#"
                UPDATE users SET email = ?1
                WHERE username = ?2;
            "#,
            email,
            username,
        )
        .execute(self.pool)
        .await?;
        if res.rows_affected() != 1 {
            Err(anyhow!("Couldn't find user with name {}", username))
        } else {
            Ok(())
        }
    }

    /// Returns Ok(Some) on success, Ok(None) on not-found.
    pub async fn destroy(&self, id: i64) -> anyhow::Result<Option<()>> {
        let res = query!(
            r#"
                DELETE FROM users WHERE id = ?;
            "#,
            id,
        )
        .execute(self.pool)
        .await?;
        if res.rows_affected() == 1 {
            Ok(Some(()))
        } else {
            Ok(None)
        }
    }
}
