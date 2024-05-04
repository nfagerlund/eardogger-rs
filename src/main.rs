mod app;
mod args;
mod config;
mod db;
mod util;

use db::Db;
use sqlx::{
    pool::PoolOptions,
    sqlite::{Sqlite, SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous},
    SqlitePool,
};
use std::{path::Path, sync::Arc, time::Duration};
use tokio::fs::{self, File};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::select;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tower_cookies::Key;
use tracing::{error, info, info_span};
use tracing_subscriber::{
    fmt::layer as fmt_layer, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter,
};

use crate::app::{eardogger_app, load_templates, state::*};
use crate::config::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // tokio console stuff:
    // - re-enable the console-subscriber dep
    // - need unstable features, so RUSTFLAGS="--cfg tokio_unstable" cargo build
    // - need `tokio=trace,runtime=trace` (in RUST_LOG or default filter)
    // let console_layer = console_subscriber::spawn(); // default values
    // .with(console_layer)
    // all this is onerous enough that I'm inclined to not leave it enabled.

    // Get args
    let options = args::cli_options();

    // Get the config
    let config = match options.config {
        Some(path) => DogConfig::load(path)?,
        None => DogConfig::load("eardogger.toml")?,
    };

    // Set up tracing. TODO: log file appender from config
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(tracing_tracy::TracyLayer::default())
        .with(fmt_layer())
        .init();

    // Set up cancellation and task tracking
    let cancel_token = CancellationToken::new();
    let tracker = TaskTracker::new();

    // Set up the database connection pool
    let cores = std::thread::available_parallelism()?.get() as u32;
    // This is a low-traffic service running on shared hardware, so go easy on parallelism.
    // Up to (cores - 2) threads, with a minimum of 2.
    let max_readers = cores.saturating_sub(2).max(2);
    let read_pool = db_pool(&config.db_file, max_readers).await?;
    let write_pool = db_pool(&config.db_file, 1).await?;
    let db = Db::new(read_pool, write_pool, tracker.clone());
    // Maybe check the migrations.
    if config.validate_migrations {
        info!("validating database migrations");
        db.validate_migrations().await?;
    }

    // Set up the cookie key
    let key = load_cookie_key(&config.key_file).await?;

    // Build the app state
    let templates = load_templates()?;
    let inner = DSInner {
        db: db.clone(),
        config: config.clone(),
        templates,
        cookie_key: key,
        task_tracker: tracker.clone(),
        cancel_token: cancel_token.clone(),
    };
    let state: DogState = Arc::new(inner);

    // ok, ok,...
    let app = eardogger_app(state);

    // Spawn the shutdown signal listener, outside the tracker
    tokio::spawn(cancel_on_terminate(cancel_token.clone()));

    // Spawn the stale session pruning worker, in the tracker
    tracker.spawn(prune_stale_sessions_worker(
        db.clone(),
        cancel_token.clone(),
    ));

    // Serve the website til we're done!
    info!("starting main server loop");
    let listener = TcpListener::bind(("0.0.0.0", config.port)).await?;
    let serve_result = axum::serve(listener, app)
        .with_graceful_shutdown(cancel_token.clone().cancelled_owned())
        .await;

    // Clean up:
    if let Err(e) = serve_result {
        // It's possible there was no cancel signal sent earlier, so send one now.
        error!("server loop exited with an error: {}", e);
        cancel_token.cancel();
    }
    info!("waiting for tasks to finish");
    tracker.close();
    tracker.wait().await;
    db.read_pool.close().await;
    db.write_pool.close().await;
    info!("see ya!");

    Ok(())
}

/// Either load the cookie key from a binary file, or create one.
async fn load_cookie_key(path: impl AsRef<Path>) -> tokio::io::Result<Key> {
    let path = path.as_ref();
    if fs::try_exists(path).await? {
        info!("loading existing cookie keyfile at {:?}", path);
        let mut f = File::open(path).await?;
        let mut keybuf = [0u8; 64];
        f.read_exact(&mut keybuf).await?;
        let key = Key::from(&keybuf);
        Ok(key)
    } else {
        info!("generating new cookie keyfile at {:?}", path);
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

async fn db_pool(
    db_file: impl AsRef<Path>,
    max_connections: u32,
) -> Result<SqlitePool, sqlx::Error> {
    let db_opts = SqliteConnectOptions::new();
    let db_opts = db_opts
        .filename(db_file)
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

/// Waits until the program receives an external instruction to terminate
/// via either SIGINT (ctrl-c) or SIGTERM (kill), then cancels the provided
/// CancellationToken. This can be spawned as an independent task, and then
/// the main logic can just await the cancellation token.
async fn cancel_on_terminate(cancel_token: CancellationToken) {
    let span = info_span!("cancel_on_terminate");
    use tokio::signal::{
        ctrl_c,
        unix::{signal, SignalKind},
    };
    let Ok(mut terminate) = signal(SignalKind::terminate()) else {
        // If we can't listen for the signal, bail immediately
        error!(parent: &span, "couldn't even establish SIGTERM signal listener; taking my ball and going home");
        cancel_token.cancel();
        return;
    };
    // Wait indefinitely until we hear a shutdown signal.
    // The ctrl_c function listens for SIGINT, the other one listens for SIGTERM
    // (aka `kill`/`killall` with no flags).
    select! {
        _ = ctrl_c() => {
            // don't care if Ok or Err
            info!(parent: &span, "received SIGINT, starting shutdown");
        },
        _ = terminate.recv() => {
            // don't care if Some or None
            info!(parent: &span, "received SIGTERM, starting shutdown");
        },
    }
    // Ok, spread the news
    cancel_token.cancel();
}

/// Long-running job to purge expired login sessions from the database,
/// so they don't keep accumulating indefinitely. This isn't
/// important enough to block any other interesting work (the queries
/// all exclude expired sessions, so they're already functionally
/// gone), but you want to do it often enough that it's always fast.
/// About the timing: if our process is owned by a web server, we're gonna
/// need to serve requests immediately upon wakeup, and some of them may
/// want the db writer. So we want to delay the first purge for several seconds.
async fn prune_stale_sessions_worker(db: Db, cancel_token: CancellationToken) {
    let span = info_span!("prune_stale_sessions_worker");
    info!(parent: &span, "starting up session pruning worker; pausing before first purge");
    let a_day = Duration::from_secs(60 * 60 * 24);
    // Initial delay (or fast-track it on cancel)
    select! {
        _ = tokio::time::sleep(Duration::from_secs(10)) => {},
        _ = cancel_token.cancelled() => {},
    }
    loop {
        info!(parent: &span, "purging stale sessions...");
        match db.sessions().delete_expired().await {
            Ok(count) => {
                info!(parent: &span, "purged {} sessions, going back to sleep", count);
            }
            Err(e) => {
                error!(
                    parent: &span,
                    "db write error while purging sessions: {}; better luck next time",
                    e
                );
            }
        }
        select! {
            // We don't really need to do this more than once a day.
            _ = tokio::time::sleep(a_day) => {}, // keep loopin'
            _ = cancel_token.cancelled() => {
                // don't keep loopin'
                break;
            }
        }
    }
    info!(parent: &span, "shutting down session pruning worker");
}
