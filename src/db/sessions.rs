use super::{core::Db, users::User};
use crate::util::{uuid_string, COOKIE_SESSION};
use sqlx::{query, query_as, SqlitePool};
use time::Duration;
use time::OffsetDateTime;
use tower_cookies::cookie::{Cookie, SameSite};
use tracing::error;

/// The max duration a session cookie can idle between logins before
/// it expires. We use a rolling window, so any logged-in activity
/// resets the expiry timer.
const SESSION_LIFETIME_DAYS: i64 = 90;

/// A query helper type for operating on [Session]s.
#[derive(Debug)]
pub struct Sessions<'a> {
    db: &'a Db,
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
    /// <https://cheatsheetseries.owasp.org/cheatsheets/Cross-Site_Request_Forgery_Prevention_Cheat_Sheet.html>
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
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }
    fn read_pool(&self) -> &SqlitePool {
        &self.db.read_pool
    }
    fn write_pool(&self) -> &SqlitePool {
        &self.db.write_pool
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
        .execute(self.write_pool())
        .await
        .map_err(|e| e.into())
        .map(|v| v.rows_affected())
    }

    /// Make a new user login session
    #[tracing::instrument]
    pub async fn create(&self, user_id: i64) -> anyhow::Result<Session> {
        let sessid = uuid_string();
        let csrf_token = uuid_string();
        let new_expires = OffsetDateTime::now_utc() + Duration::days(SESSION_LIFETIME_DAYS);
        // ^^ theoretically I could stack-allocate that but ehhhh
        query_as!(
            Session,
            r#"
                INSERT INTO sessions (id, user_id, csrf_token, expires)
                VALUES (?1, ?2, ?3, datetime(?4))
                RETURNING id, user_id, csrf_token, expires;
            "#,
            sessid,
            user_id,
            csrf_token,
            new_expires,
        )
        .fetch_one(self.write_pool())
        .await
        .map_err(|e| e.into())
    }

    /// Returns Ok(Some) on success, Ok(None) on a well-behaved not-found.
    #[tracing::instrument(skip_all)]
    pub async fn destroy(&self, sessid: &str) -> anyhow::Result<Option<()>> {
        let res = query!(
            r#"
                DELETE FROM sessions
                WHERE id = ?;
            "#,
            sessid,
        )
        .execute(self.write_pool())
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
    #[tracing::instrument(skip_all)]
    pub async fn authenticate(&self, sessid: &str) -> anyhow::Result<Option<(Session, User)>> {
        let new_expires = OffsetDateTime::now_utc() + Duration::days(SESSION_LIFETIME_DAYS);

        // First, get the stuff
        let maybe = query!(
            r#"
                SELECT
                    sessions.id         AS session_id,
                    sessions.user_id    AS user_id,
                    sessions.csrf_token AS session_csrf_token,
                    users.username      AS user_username,
                    users.email         AS user_email,
                    users.created       AS user_created
                FROM sessions JOIN users ON sessions.user_id = users.id
                WHERE sessions.id = ?1 AND sessions.expires > datetime('now');
            "#,
            sessid,
        )
        .fetch_optional(self.read_pool())
        .await?;

        // Early out if we got nuthin; this also skips the async update.
        let Some(stuff) = maybe else {
            return Ok(None);
        };

        // Then, do a fire-and-forget update; we don't need to see the result in
        // our read. This lets us skip waiting for the single
        // writer thread in the warm path of "doing literally anything logged in."
        let write_pool = self.write_pool().clone();
        let owned_sessid = sessid.to_string();
        self.db.task_tracker.spawn(async move {
            let q_res = query!(
                r#"
                    UPDATE sessions SET expires = datetime(?1)
                    WHERE id = ?2 AND expires > datetime('now');
                "#,
                new_expires,
                owned_sessid,
            )
            .execute(&write_pool)
            .await;

            if let Err(e) = q_res {
                error!(
                    name: "Sessions::authenticate expiry update",
                    "DB write failed for async update of session expiration: {}",
                    e,
                );
            }
        });

        // Finally, assemble the stuff. sessions.expires is being updated async with the
        // pre-calculated value, so we ignore the stored value and just return that.
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
            expires: new_expires,
        };
        Ok(Some((session, user)))
    }
}
