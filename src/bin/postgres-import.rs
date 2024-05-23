use sqlx::sqlite::{SqliteJournalMode, SqliteSynchronous};
use sqlx::{
    postgres::PgConnectOptions, sqlite::SqliteConnectOptions, ConnectOptions, Connection, Pool,
    Postgres, Sqlite,
};
use sqlx::{PgConnection, SqliteConnection};
use std::env;
use std::str::FromStr;
use std::time::Duration;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    println!("hi.");
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
async fn databases(options: Options) -> (SqliteConnection, PgConnection) {
    let lite = SqliteConnectOptions::from_str(&options.sqlite_url)
        .unwrap()
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5))
        .pragma("temp_store", "memory")
        .optimize_on_close(true, 400)
        .synchronous(SqliteSynchronous::Normal) // usually fine w/ wal
        .foreign_keys(true)
        .connect()
        .await
        .unwrap();
    let post = PgConnectOptions::from_str(&options.postgres_url)
        .unwrap()
        .connect()
        .await
        .unwrap();

    (lite, post)
}
