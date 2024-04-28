mod app;
mod db;
mod util;

use db::Db;
use sqlx::{
    pool::PoolOptions,
    sqlite::{Sqlite, SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous},
    SqlitePool,
};
use std::time::Duration;
use std::{str::FromStr, sync::Arc};
use tokio::fs::{self, File};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tower_cookies::Key;
use tracing_subscriber::{
    fmt::layer as fmt_layer, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter,
};
use url::Url;

use crate::app::{eardogger_app, load_templates, state::*};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // tokio console stuff:
    // - re-enable the console-subscriber dep
    // - need unstable features, so RUSTFLAGS="--cfg tokio_unstable" cargo build
    // - need `tokio=trace,runtime=trace` (in RUST_LOG or default filter)
    // let console_layer = console_subscriber::spawn(); // default values
    // .with(console_layer)
    // all this is onerous enough that I'm inclined to not leave it enabled.

    // Set up tracing
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(tracing_tracy::TracyLayer::default())
        .with(fmt_layer())
        .init();
    // Set up the database connection pool
    // TODO: extract DB url into config
    let db_url = "sqlite:dev.db";
    let cores = std::thread::available_parallelism()?.get() as u32;
    // This is a low-traffic service running on shared hardware, so go easy on parallelism.
    // Up to (cores - 2) threads, with a minimum of 2.
    let max_readers = cores.saturating_sub(2).max(2);
    let pool = db_pool(db_url, max_readers).await?;
    // TODO: migrations?

    // Set up the cookie key
    // TODO: extract keyfile path into config
    let key_file = "cookie_key.bin";
    let key = load_cookie_key(key_file).await?;

    // Build the app state and config
    // TODO: extract all this into more convenient... stuffs...
    let db = Db::new(pool);
    // TODO: get own_origin and assets_dir from config instead
    let own_url = Url::parse("http://localhost:3000")?;
    let assets_dir = "public".to_string();
    let config = DogConfig {
        is_prod: false,
        own_url,
        assets_dir,
    };
    let templates = load_templates()?;
    let inner = DSInner {
        db,
        config,
        templates,
        cookie_key: key,
    };
    let state: DogState = Arc::new(inner);

    // ok, ok,...
    let app = eardogger_app(state);

    // TODO: get network stuff from config, do multi-modal serving
    let listener = TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// Either load the cookie key from a binary file, or create one. IRL you'd want the file location
/// to come from config somewhere, but I'm gonna hardcode it in cwd.
async fn load_cookie_key(path: &str) -> tokio::io::Result<Key> {
    if fs::try_exists(path).await? {
        let mut f = File::open(path).await?;
        let mut keybuf = [0u8; 64];
        f.read_exact(&mut keybuf).await?;
        let key = Key::from(&keybuf);
        Ok(key)
    } else {
        let mut f = File::options()
            .write(true)
            .create_new(true)
            .open(path)
            .await?;
        // generate() uses thread_rng(), which is what I'd have done manually anyway.
        let key = Key::generate();
        // save it for later
        f.write_all(key.master()).await?;
        Ok(key)
    }
}

async fn db_pool(db_url: &str, max_connections: u32) -> Result<SqlitePool, sqlx::Error> {
    let db_opts = SqliteConnectOptions::from_str(db_url)?;
    let db_opts = db_opts
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5))
        .pragma("temp_store", "memory")
        .optimize_on_close(true, 400)
        .synchronous(SqliteSynchronous::Normal) // usually fine w/ wal
        .foreign_keys(true);
    let pool_opts: PoolOptions<Sqlite> = PoolOptions::new()
        .max_connections(max_connections) // default's 10, but we'll be explicit.
        .min_connections(1)
        // boss makes a dollar, db thread makes a dime, that's why I fish crab on company time
        .max_lifetime(Duration::from_secs(60 * 60 * 4));
    pool_opts.connect_with(db_opts).await
}
