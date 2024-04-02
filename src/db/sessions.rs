use super::users::User;
use crate::util::{uuid_string, COOKIE_SESSION};
use sqlx::{query, query_as, SqlitePool};
use time::OffsetDateTime;
use tower_cookies::cookie::{Cookie, SameSite};

/// The max duration a session cookie can idle between logins before
/// it expires. Because of sqlite's phantom date/time types, this must
/// be passed as a whole string param, not interpolated from an int.
const SESSION_LIFETIME_MODIFIER: &str = "+90 days";

/// A query helper type for operating on [Session]s. Usually rented from a [Db].
#[derive(Debug)]
pub struct Sessions<'a> {
    pool: &'a SqlitePool,
}

/// A record struct for user login sessions.
#[derive(Debug, Clone)]
pub struct Session {
    /// An opaque, securely-random ID string (actually a UUIDv4). Stored as
    /// a cookie in the user's browser and used to look up the session from the db.
    pub id: String,
    pub user_id: i64,
    /// An opaque, securely-random garbage string (actually a UUIDv4) to be included
    /// as a hidden input in "plain" HTML forms presented to this user on this
    /// session. On submit, the posted value must match the saved session value;
    /// a mismatch would mean they submitted something *other* than a form we presented
    /// them on purpose. For the logged-out login form, we do this with a different
    /// scheme involving signed cookies. For more info, see:
    /// https://cheatsheetseries.owasp.org/cheatsheets/Cross-Site_Request_Forgery_Prevention_Cheat_Sheet.html
    pub csrf_token: String,
    pub expires: OffsetDateTime,
}

impl Session {
    /// Consume a session to bake a cookie.
    pub fn into_cookie(self) -> Cookie<'static> {
        let Self { id, expires, .. } = self;
        Cookie::build((COOKIE_SESSION, id))
            .expires(expires)
            .http_only(true)
            .secure(true)
            // RIP cookie auth in bookmarklets:
            .same_site(SameSite::Lax)
            .build()
            .into_owned()
    }
}

// create, authenticate, destroy, delete_expired
impl<'a> Sessions<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Delete all expired sessions from the database. This is a low-priority
    /// cleanup operation that should happen as a background task rather than
    /// blocking a user request... but it should happen fairly often so the
    /// number of sessions to waste at once never gets very large.
    pub async fn delete_expired(&self) -> anyhow::Result<u64> {
        // y'know, ideally I would like to set a limit for how many
        // records to waste at a time, just to guard against blowouts...
        // but it's behind the SQLITE_ENABLE_UPDATE_DELETE_LIMIT compile-time
        // option and IDK if that's available in sqlx's bundled build.
        query!(
            r#"
                DELETE FROM sessions WHERE expires < datetime('now');
            "#
        )
        .execute(self.pool)
        .await
        .map_err(|e| e.into())
        .map(|v| v.rows_affected())
    }

    /// Make a new user login session
    #[tracing::instrument]
    pub async fn create(&self, user_id: i64) -> anyhow::Result<Session> {
        let sessid = uuid_string();
        let csrf_token = uuid_string();
        // ^^ theoretically I could stack-allocate that but ehhhh
        query_as!(
            Session,
            r#"
                INSERT INTO sessions (id, user_id, csrf_token, expires)
                VALUES (?1, ?2, ?3, datetime('now', ?4))
                RETURNING id, user_id, csrf_token, expires;
            "#,
            sessid,
            user_id,
            csrf_token,
            SESSION_LIFETIME_MODIFIER,
        )
        .fetch_one(self.pool)
        .await
        .map_err(|e| e.into())
    }

    /// Returns Ok(Some) on success, Ok(None) on a well-behaved not-found.
    #[tracing::instrument]
    pub async fn destroy(&self, sessid: &str) -> anyhow::Result<Option<()>> {
        let res = query!(
            r#"
                DELETE FROM sessions
                WHERE id = ?;
            "#,
            sessid,
        )
        .execute(self.pool)
        .await?;
        if res.rows_affected() == 1 {
            Ok(Some(()))
        } else {
            Ok(None)
        }
    }

    /// Find the user and session for a given session ID (IF the session is
    /// still valid). As a side-effect, updates the session's expiration date
    /// to maintain the rolling window.
    #[tracing::instrument]
    pub async fn authenticate(&self, sessid: &str) -> anyhow::Result<Option<(Session, User)>> {
        // First do a fire-and-forget update, it's fine if it whiffs.
        query!(
            r#"
                UPDATE sessions SET expires = datetime('now', ?1)
                WHERE id = ?2 AND expires > datetime('now');
            "#,
            SESSION_LIFETIME_MODIFIER,
            sessid,
        )
        .execute(self.pool)
        .await?;

        // Get the goods!!
        let maybe = query!(
            r#"
                SELECT
                    sessions.id         AS session_id,
                    sessions.user_id    AS user_id,
                    sessions.csrf_token AS session_csrf_token,
                    sessions.expires    AS session_expires,
                    users.username      AS user_username,
                    users.email         AS user_email,
                    users.created       AS user_created
                FROM sessions JOIN users ON sessions.user_id = users.id
                WHERE sessions.id = ?1 AND sessions.expires > datetime('now');
            "#,
            sessid,
        )
        .fetch_optional(self.pool)
        .await?;

        if let Some(stuff) = maybe {
            let user = User {
                id: stuff.user_id,
                username: stuff.user_username,
                email: stuff.user_email,
                created: stuff.user_created,
            };
            let session = Session {
                id: stuff.session_id,
                user_id: stuff.user_id,
                csrf_token: stuff.session_csrf_token,
                expires: stuff.session_expires,
            };
            Ok(Some((session, user)))
        } else {
            Ok(None)
        }
    }
}
