use anyhow::anyhow;
use lazy_static::lazy_static;
use regex::Regex;
use sqlx::{query_as, SqlitePool};
use time::OffsetDateTime;

/// A query helper type for operating on [User]s. Usually you rent this from
/// a [Db].
pub struct Users<'a> {
    pool: &'a SqlitePool,
}

/// Record struct for user accounts.
#[derive(Debug, PartialEq, Clone)]
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
/// We want to be able to omit email, but HTML forms make that tricky. So, flatmap it!
fn clean_email(email: Option<&str>) -> Option<&str> {
    email.and_then(|e| {
        let e = e.trim();
        if e.is_empty() {
            None
        } else {
            Some(e)
        }
    })
}
fn valid_password(password: &str) -> anyhow::Result<&str> {
    if password.is_empty() {
        Err(anyhow!("Empty password isn't allowed."))
    } else {
        Ok(password)
    }
}

impl<'a> Users<'a> {
    /// Create a new user account.
    pub async fn create(
        &self,
        username: &str,
        password: &str,
        email: Option<&str>,
    ) -> anyhow::Result<User> {
        let username = clean_username(username)?;
        let email = clean_email(email);
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

    /// Authenticate a user by username and password. Only returns Some if the
    /// user exists and the password matches.
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
}
