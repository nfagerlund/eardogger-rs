//! This script is for migrating data between an eardogger 1 postgres database
//! and an eardogger 2 sqlite database.
//!
//! - Both databases are expected to be in a working condition! You need to
//! run all your migrations or whatever out of band before trying this.
//!
//! - We preserve users, tokens, and dogears. We don't preserve login sessions,
//! so everyone will have to log in again after cutting over to the new (or old)
//! instance.
//!
//! - We don't preserve numeric IDs for objects. We DO rely on uniqueness
//! constraints on usernames and (prefix, userid) tuples.

use futures_util::stream::TryStreamExt;
use sqlx::{
    pool::PoolOptions,
    postgres::PgConnectOptions,
    query, query_as,
    sqlite::SqliteConnectOptions,
    sqlite::{SqliteJournalMode, SqliteSynchronous},
    FromRow, PgPool, SqlitePool,
};
use std::env;
use std::str::FromStr;
use std::time::Duration;
use time::OffsetDateTime;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let time_of_import = OffsetDateTime::now_utc();
    let (v2sqlite, v1postgres) = databases(&parse_args()).await;
    // I'm gonna just yolo it on the resource usage here -_- probably
    // I should be handling batching but, pain in the butt, and the data be small.
    let mut user_stream = query_as::<_, V1User>(
        r#"
            SELECT id, username, password, email, created
            FROM users;
        "#,
    )
    .fetch(&v1postgres);
    // Do each user import in a transaction.
    while let Some(v1user) = user_stream.try_next().await.unwrap() {
        // Start a per-user transaction
        let mut tx = v2sqlite.begin().await.unwrap();
        // insert user
        let (v2_user_id, username) = query_as::<_, (i64, String)>(
            r#"
                INSERT INTO users (username, password_hash, email, created)
                VALUES (?1, ?2, ?3, ?4)
                ON CONFLICT(username) DO UPDATE
                    SET password_hash = ?2, email = ?3, created = ?4
                RETURNING id, username;
            "#,
        )
        .bind(&v1user.username)
        // NOT NULL is new, so absent password hash becomes empty string
        .bind(v1user.password.as_deref().unwrap_or(""))
        .bind(v1user.email.as_ref())
        .bind(v1user.created.as_ref().unwrap_or(&time_of_import))
        .fetch_one(&mut *tx)
        .await
        .unwrap();

        // query tokens
        let mut tokens_stream = query_as::<_, V1Token>(
            r#"
                SELECT id, user_id, token_hash, scope, created, comment, last_used
                FROM tokens
                WHERE user_id = $1;
            "#,
        )
        .bind(v1user.id)
        .fetch(&v1postgres);
        while let Some(v1token) = tokens_stream.try_next().await.unwrap() {
            // Lil guardrail... I'm not concerned about user_id, because we're already
            // guarded by a where clause above.
            if v1token.token_hash.is_none() {
                continue;
            }
            query(
                r#"
                    INSERT INTO tokens (user_id, token_hash, scope, created, comment, last_used)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                    ON CONFLICT(token_hash) DO NOTHING;
                "#,
            )
            .bind(v2_user_id)
            .bind(v1token.token_hash.as_ref().unwrap())
            .bind(
                v1token
                    .scope
                    .as_ref()
                    .map(|s| s.to_str())
                    .unwrap_or("invalid"),
            )
            .bind(v1token.created.as_ref().unwrap_or(&time_of_import))
            .bind(&v1token.comment)
            .bind(v1token.last_used)
            .execute(&mut *tx)
            .await
            .unwrap();
        }

        // query dogears
        let mut dogears_stream = query_as::<_, V1Dogear>(
            r#"
                SELECT id, user_id, prefix, current, display_name, updated
                FROM dogears
                WHERE user_id = $1;
            "#,
        )
        .bind(v1user.id)
        .fetch(&v1postgres);
        while let Some(v1dogear) = dogears_stream.try_next().await.unwrap() {
            // lil guardrail
            if v1dogear.current.is_none() {
                continue;
            }
            query(
                r#"
                    INSERT INTO dogears (user_id, prefix, current, display_name, updated)
                    VALUES (?1, ?2, ?3, ?4, ?5)
                    ON CONFLICT(user_id, prefix) DO UPDATE
                        SET current = ?3, display_name = ?4, updated = ?5;
                "#,
            )
            .bind(v2_user_id)
            .bind(&v1dogear.prefix)
            .bind(v1dogear.current.as_ref().unwrap())
            .bind(&v1dogear.display_name)
            .bind(v1dogear.updated.as_ref().unwrap_or(&time_of_import))
            .execute(&mut *tx)
            .await
            .unwrap();
        }

        // that's a wrap
        tx.commit().await.unwrap();
    }
}

#[derive(FromRow)]
struct V1User {
    id: i32, // INT4
    username: String,
    password: Option<String>,
    email: Option<String>,
    created: Option<OffsetDateTime>,
}

#[derive(FromRow)]
struct V1Token {
    id: i32,              // INT4
    user_id: Option<i32>, // oh lol yikes // INT4
    token_hash: Option<String>,
    scope: Option<V1TokenScope>,
    created: Option<OffsetDateTime>,
    comment: Option<String>,
    last_used: Option<OffsetDateTime>,
}

#[derive(FromRow)]
struct V1Dogear {
    id: i32,      // INT4
    user_id: i32, // INT4
    prefix: String,
    current: Option<String>,
    display_name: Option<String>,
    updated: Option<OffsetDateTime>,
}

#[allow(non_camel_case_types)]
#[derive(sqlx::Type)]
#[sqlx(type_name = "token_scope", rename_all = "lowercase")]
enum V1TokenScope {
    Manage_Dogears,
    Write_Dogears,
}

impl V1TokenScope {
    fn to_str(&self) -> &'static str {
        match self {
            V1TokenScope::Manage_Dogears => "manage_dogears",
            V1TokenScope::Write_Dogears => "write_dogears",
        }
    }

    fn from_str(v: &str) -> Option<Self> {
        match v {
            "manage_dogears" => Some(Self::Manage_Dogears),
            "write_dogears" => Some(Self::Write_Dogears),
            _ => None, // the postgres type doesn't have an "invalid" variant, so.
        }
    }
}

#[derive(FromRow)]
struct V2User {
    id: i64,
    username: String,
    password_hash: String,
    email: Option<String>,
    created: OffsetDateTime,
}

#[derive(FromRow)]
struct V2Token {
    id: i64,
    user_id: i64,
    token_hash: String,
    scope: String,
    created: OffsetDateTime,
    comment: Option<String>,
    last_used: Option<OffsetDateTime>,
}

#[derive(FromRow)]
struct V2Dogear {
    id: i64,
    user_id: i64,
    prefix: String,
    current: String,
    display_name: Option<String>,
    updated: OffsetDateTime,
    // UNIQUE (user_id, prefix) ON CONFLICT ROLLBACK
}

struct Options {
    postgres_url: String,
    sqlite_url: String,
}

#[derive(Debug)]
enum ArgsParseState {
    Scanning,
    PostgresVal,
    SqliteVal,
}

/// Grab the options off the CLI.
fn parse_args() -> Options {
    let mut postgres_url: Option<String> = None;
    let mut sqlite_url: Option<String> = None;
    let mut state = ArgsParseState::Scanning;

    for arg in env::args() {
        match state {
            ArgsParseState::Scanning => {
                if arg == "--postgres_url" {
                    state = ArgsParseState::PostgresVal;
                } else if arg == "--sqlite_url" {
                    state = ArgsParseState::SqliteVal;
                }
            }
            ArgsParseState::PostgresVal => {
                postgres_url = Some(arg);
                state = ArgsParseState::Scanning;
            }
            ArgsParseState::SqliteVal => {
                sqlite_url = Some(arg);
                state = ArgsParseState::Scanning;
            }
        }
    }
    Options {
        postgres_url: postgres_url
            .expect("Usage: postgres-import --postgres_url <URL> --sqlite_url <URL>"),
        sqlite_url: sqlite_url
            .expect("Usage: postgres-import --postgres_url <URL> --sqlite_url <URL>"),
    }
}

/// Set up database connections. Panics on failure, because we're just a lil baby script.
async fn databases(options: &Options) -> (SqlitePool, PgPool) {
    let lite_opts = SqliteConnectOptions::from_str(&options.sqlite_url)
        .unwrap()
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5))
        .pragma("temp_store", "memory")
        .optimize_on_close(true, 400)
        .synchronous(SqliteSynchronous::Normal) // usually fine w/ wal
        .foreign_keys(true);
    let post_opts = PgConnectOptions::from_str(&options.postgres_url).unwrap();

    // Setting max_connections to 2 so we can do nested read streams.
    let lite = PoolOptions::new()
        .max_connections(2)
        .connect_with(lite_opts)
        .await
        .unwrap();
    let post = PoolOptions::new()
        .max_connections(2)
        .connect_with(post_opts)
        .await
        .unwrap();

    (lite, post)
}
