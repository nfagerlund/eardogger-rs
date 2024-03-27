use super::users::User;
use crate::util::{sha256sum, sqlite_offset, uuid_string, ListMeta};
use anyhow::anyhow;
use serde::Serialize;
use sqlx::{query, query_as, SqlitePool};
use time::{serde::iso8601, OffsetDateTime};

/// A query helper type for operating on [Token]s. Usually rented from a [Db].
#[derive(Debug)]
pub struct Tokens<'a> {
    pool: &'a SqlitePool,
}

/// Record struct for API authentication tokens associated with a [User].
/// Unlike User passwords, tokens can't be chosen by a user or re-used
/// elsewhere, so they don't need a time-wasting hash function like bcrypt
/// or argon2. We still don't store the token cleartext itself, but we
/// just hash it with plain old unsalted sha256. Sometimes the classics
/// are best.
#[derive(Debug, Clone, Serialize)]
pub struct Token {
    pub id: i64,
    pub user_id: i64,
    scope: String, // private, use .scope().
    #[serde(with = "iso8601")]
    pub created: OffsetDateTime,
    #[serde(with = "iso8601::option")]
    pub last_used: Option<OffsetDateTime>,
    pub comment: Option<String>,
    // notably excluded: token_hash and also the temporary cleartext.
}

impl Token {
    pub fn scope(&self) -> TokenScope {
        self.scope.as_str().into()
    }
}

impl PartialEq for Token {
    fn eq(&self, other: &Self) -> bool {
        // Skip last_used bc it can change on auth-fetch
        self.id == other.id
            && self.user_id == other.user_id
            && self.scope == other.scope
            && self.created == other.created
            && self.comment == other.comment
    }
}

/// The exhaustive list of full permission types that API tokens can have.
/// These values are stored in the database as text, but the application
/// code can have a little enum. as a treat.
#[derive(PartialEq, Eq, Clone, Debug)]
pub enum TokenScope {
    /// Text: `write_dogears`.
    /// Can POST `/api/v1/create` and `/api/v1/update`.
    WriteDogears,
    /// Text: `manage_dogears`.
    /// Can POST `/api/v1/create` and `/api/v1/update`.
    /// Can GET `/api/v1/list`.
    /// Can DELETE `/api/v1/dogear/:id`.
    ManageDogears,
    /// Can't do shit!!
    Invalid,
}

impl From<&str> for TokenScope {
    fn from(value: &str) -> Self {
        match value {
            "write_dogears" => Self::WriteDogears,
            "manage_dogears" => Self::ManageDogears,
            _ => Self::Invalid,
        }
    }
}

impl From<TokenScope> for &'static str {
    fn from(value: TokenScope) -> Self {
        match value {
            TokenScope::WriteDogears => "write_dogears",
            TokenScope::ManageDogears => "manage_dogears",
            TokenScope::Invalid => "INVALID",
        }
    }
}

// create, authenticate, destroy, list
impl<'a> Tokens<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Create a token, and return it along with the *actual token cleartext.*
    /// This is the only time the cleartext is ever available.
    #[tracing::instrument]
    pub async fn create(
        &self,
        user_id: i64,
        scope: TokenScope,
        comment: Option<&str>,
    ) -> anyhow::Result<(Token, String)> {
        let token_cleartext = format!("eardoggerv1.{}", uuid_string());
        let token_hash = sha256sum(&token_cleartext);
        let scope_str: &str = scope.into();
        let token = query_as!(
            Token,
            r#"
                INSERT INTO tokens (user_id, token_hash, scope, comment)
                VALUES (?1, ?2, ?3, ?4)
                RETURNING id, user_id, scope, created, last_used, comment;
            "#,
            user_id,
            token_hash,
            scope_str,
            comment
        )
        .fetch_one(self.pool)
        .await?;

        Ok((token, token_cleartext))
    }

    /// Use the provided token cleartext to look up a token and its associated user.
    /// Returns Ok(None) if the token doesn't match anything.
    #[tracing::instrument]
    pub async fn authenticate(
        &self,
        token_cleartext: &str,
    ) -> anyhow::Result<Option<(Token, User)>> {
        let token_hash = sha256sum(token_cleartext);

        // First off, we do a fire-and-forget last-used bump. If this update
        // whiffs, that's fine.
        //
        // (I wanted to do everything in one go, but I need the user too, and
        // sqlite can't access an update's FROM clause in its RETURNING clause.)
        query!(
            r#"
                UPDATE tokens
                SET last_used = CURRENT_TIMESTAMP
                WHERE token_hash = ?;
            "#,
            token_hash
        )
        .execute(self.pool)
        .await?;

        // Use query!() instead of query_as!(), because we want multiple records
        // and we don't have a struct for "user plus token fields".
        let maybe = query!(
            r#"
                SELECT
                    tokens.id        AS token_id,
                    tokens.user_id   AS user_id,
                    tokens.scope     AS token_scope,
                    tokens.created   AS token_created,
                    tokens.last_used AS token_last_used,
                    tokens.comment   AS token_comment,
                    users.username   AS user_username,
                    users.email      AS user_email,
                    users.created    AS user_created
                FROM tokens JOIN users ON tokens.user_id = users.id
                WHERE tokens.token_hash = ? LIMIT 1;
            "#,
            token_hash
        )
        .fetch_optional(self.pool)
        .await?;

        // Early out if we got nuthin
        let Some(stuff) = maybe else {
            return Ok(None);
        };
        let token = Token {
            id: stuff.token_id,
            user_id: stuff.user_id,
            scope: stuff.token_scope,
            created: stuff.token_created,
            last_used: stuff.token_last_used,
            comment: stuff.token_comment,
        };
        let user = User {
            id: stuff.user_id,
            username: stuff.user_username,
            email: stuff.user_email,
            created: stuff.user_created,
        };
        Ok(Some((token, user)))
    }

    /// Delete a token. To double-check the permissions, get the token's
    /// user ID from a trusted source and provide it when calling this.
    /// Returns Err on database problems, Ok(None) if db's ok but there's
    /// nothing to delete.
    #[tracing::instrument]
    pub async fn destroy(&self, id: i64, user_id: i64) -> anyhow::Result<Option<()>> {
        let res = query!(
            r#"
                DELETE FROM tokens
                WHERE id = ?1 AND user_id = ?2;
            "#,
            id,
            user_id,
        )
        .execute(self.pool)
        .await?;
        if res.rows_affected() == 1 {
            Ok(Some(()))
        } else {
            Ok(None)
        }
    }

    /// List some of a user's tokens, with an adjustable page size.
    #[tracing::instrument]
    pub async fn list(
        &self,
        user_id: i64,
        page: u32,
        size: u32,
    ) -> anyhow::Result<(Vec<Token>, ListMeta)> {
        // Get count first, as a separate query. For some reason sqlx tries
        // by default to return the value of COUNT() as an i32, which I
        // KNOW is not correct, so that column name with a colon overrides it
        // at the sqlx layer. I think.
        let count = query!(
            r#"
                SELECT COUNT(id) AS 'count: u32' FROM tokens WHERE user_id = ?;
            "#,
            user_id,
        )
        .fetch_one(self.pool)
        .await?
        .count;

        let offset = sqlite_offset(page, size)?;
        let meta = ListMeta { count, page, size };
        let list = query_as!(
            Token,
            r#"
                SELECT id, user_id, scope, created, last_used, comment
                FROM tokens
                WHERE user_id = ?1
                ORDER BY last_used DESC NULLS LAST, id DESC
                LIMIT ?2
                OFFSET ?3;
            "#,
            user_id,
            size,
            offset,
        )
        .fetch_all(self.pool)
        .await?;

        Ok((list, meta))
    }
}
