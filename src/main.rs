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
    // Set up tracing
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(fmt_layer())
        .init();
    // Set up the database connection pool
    // TODO: extract DB url into config
    let db_url = "sqlite:dev.db";
    let pool = db_connect(db_url).await?;
    // TODO: migrations?

    // Set up the cookie key
    // TODO: extract keyfile path into config
    let key_file = "cookie_key.bin";
    let key = load_cookie_key(key_file).await?;

    // Build the app state and config
    // TODO: extract all this into more convenient... stuffs...
    let db = Db::new(pool);
    // TODO: get own_origin and assets_dir from config instead
    let own_origin = Url::parse("http://localhost:3000")?;
    let assets_dir = "public".to_string();
    let config = DogConfig {
        is_prod: false,
        own_origin,
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

async fn db_connect(db_url: &str) -> Result<SqlitePool, sqlx::Error> {
    let db_opts = SqliteConnectOptions::from_str(db_url)?;
    let db_opts = db_opts
        .journal_mode(SqliteJournalMode::Wal)
        .optimize_on_close(true, 400)
        .synchronous(SqliteSynchronous::Normal) // usually fine w/ wal
        .foreign_keys(true);
    let pool_opts: PoolOptions<Sqlite> = PoolOptions::new()
        .max_connections(50) // default's 10, seems low
        // boss makes a dollar, db thread makes a dime, that's why I fish crab on company time
        .max_lifetime(Duration::from_secs(60 * 60 * 8));
    pool_opts.connect_with(db_opts).await
}
