use crate::util::{matchable_from_url, normalize_prefix_matcher, sqlite_offset, ListMeta};
use anyhow::anyhow;
use sqlx::{query, query_as, SqlitePool};
use time::OffsetDateTime;

/// A query helper type for operating on [Dogears]. Usually rented from a [Db].
pub struct Dogears<'a> {
    pool: &'a SqlitePool,
}

/// A record struct for user web serial bookmarks.
#[derive(Debug)]
pub struct Dogear {
    pub id: i64,
    pub user_id: i64,
    pub prefix: String,
    pub current: String,
    pub display_name: Option<String>,
    pub updated: OffsetDateTime,
}

// create, update, list, destroy, current_for_site
impl<'a> Dogears<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Make a new dogear!
    pub async fn create(
        &self,
        user_id: i64,
        prefix: &str,
        current: &str,
        display_name: Option<&str>,
    ) -> anyhow::Result<Dogear> {
        let normalized_prefix = normalize_prefix_matcher(prefix);
        // Confirm that the current URL is valid and matches the prefix
        let matchable_current = matchable_from_url(current)?;
        if !matchable_current.starts_with(normalized_prefix) {
            return Err(anyhow!(
                "The provided URL doesn't match the provided prefix."
            ));
        }

        query_as!(
            Dogear,
            r#"
                INSERT INTO dogears (user_id, prefix, current, display_name)
                VALUES (?1, ?2, ?3, ?4)
                RETURNING id, user_id, prefix, current, display_name, updated;
            "#,
            user_id,
            normalized_prefix,
            current,
            display_name
        )
        .fetch_one(self.pool)
        .await
        .map_err(|e| e.into())
    }

    /// Given a user and a current URL, update the corresponding dogear to
    /// its new location. ...Actually, because we don't do a rigorous check to
    /// ensure all prefixes are non-overlapping, this can update multiple
    /// dogears at once. That's kind of fine, though; it's some minor jank
    /// that saves us a bunch of bullshit elsewhere in the system. If you
    /// got your personal dogears into a weird situation, just delete some.
    /// Returns None if no dogears matched.
    pub async fn update(&self, user_id: i64, current: &str) -> anyhow::Result<Option<Vec<Dogear>>> {
        let matchable = matchable_from_url(current)?;
        let res = query_as!(
            Dogear,
            r#"
                UPDATE dogears
                SET current = ?1, updated = current_timestamp
                WHERE
                    user_id = ?2 AND
                    ?3 LIKE prefix || '%'
                RETURNING id, user_id, prefix, current, display_name, updated;
            "#,
            current,
            user_id,
            matchable,
        )
        .fetch_all(self.pool)
        .await?;
        if res.is_empty() {
            Ok(None)
        } else {
            Ok(Some(res))
        }
    }

    /// Given a URL and a user, return the currently bookmarked page on that site.
    /// (or None.) This partially acknowledges the "overlapping prefixes" loophole
    /// by returning the result with the *longest* matching prefix.
    pub async fn current_for_site(
        &self,
        user_id: i64,
        url: &str,
    ) -> anyhow::Result<Option<String>> {
        let matchable = matchable_from_url(url)?;
        let res = query!(
            r#"
                SELECT current
                FROM dogears
                WHERE
                    user_id = ?1 AND
                    ?2 LIKE prefix || '%'
                ORDER BY length(prefix) DESC
                LIMIT 1;
            "#,
            user_id,
            matchable,
        )
        .fetch_optional(self.pool)
        .await?;
        Ok(res.map(|r| r.current))
    }

    /// yeah
    pub async fn destroy(&self, id: i64, user_id: i64) -> anyhow::Result<()> {
        let res = query!(
            r#"
                DELETE FROM dogears
                WHERE id = ?1 AND user_id = ?2;
            "#,
            id,
            user_id,
        )
        .execute(self.pool)
        .await?;
        if res.rows_affected() == 1 {
            Ok(())
        } else {
            Err(anyhow!("Couldn't find the specified dogear."))
        }
    }

    /// List some of the user's dogears, with an adjustable page size.
    pub async fn list(
        &self,
        user_id: i64,
        page: u32,
        size: u32,
    ) -> anyhow::Result<(Vec<Dogear>, ListMeta)> {
        // Count first, as a separate query. Note the sqlx "type coersion inside
        // the column name" thing, sigh.
        let count = query!(
            r#"
                SELECT count(id) AS 'count: u32' FROM dogears
                WHERE user_id = ?;
            "#,
            user_id,
        )
        .fetch_one(self.pool)
        .await?
        .count;

        let offset = sqlite_offset(page, size)?;
        let meta = ListMeta { count, page, size };
        let list = query_as!(
            Dogear,
            r#"
                SELECT id, user_id, prefix, current, display_name, updated
                FROM dogears
                WHERE user_id = ?1
                ORDER BY updated DESC
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
