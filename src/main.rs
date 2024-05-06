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
use tracing::{debug, error, info};
use tracing_appender::{
    non_blocking::WorkerGuard,
    rolling::{RollingFileAppender, Rotation},
};
use tracing_subscriber::{
    fmt::layer as fmt_layer, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter,
};

use crate::app::{eardogger_app, load_templates, state::*};
use crate::config::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Get args
    let options = args::cli_options();

    // Get the config
    let config = match options.config {
        Some(path) => DogConfig::load(path)?,
        None => DogConfig::load("eardogger.toml")?,
    };

    // Set up tracing

    // A Registry subscriber is a hairball of a type that grows more fuzz
    // with every layer, so you can't do conditional `.with()`s. But
    // Option<Layer> implements Layer, so we can unconditionally add layer values
    // that represent a condition.
    let stdout_layer = if config.log.stdout {
        Some(fmt_layer())
    } else {
        None
    };

    // The non-blocking logfile writer relies on a drop-guard to ensure writes get
    // flushed at the end of main. So we need to make sure we're holding onto it
    // at top scope, instead of dropping it at the end of a conditional.
    let mut _log_writer_guard: Option<WorkerGuard> = None;
    let logrotate_layer = if let Some(logfile) = &config.log.file {
        let file_appender = RollingFileAppender::builder()
            .rotation(Rotation::DAILY)
            .filename_prefix(&logfile.name)
            .filename_suffix("log")
            .max_log_files(logfile.days)
            .build(&logfile.directory)?;
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
        // Hand the guard off to the outer scope
        _log_writer_guard = Some(guard);
        Some(fmt_layer().with_writer(non_blocking).with_ansi(false))
    } else {
        None
    };

    // Ok, there we go. Beyond this point, we can now log with tracing!
    tracing_subscriber::registry()
        .with(EnvFilter::new(&config.log.filter))
        .with(stdout_layer)
        .with(logrotate_layer)
        .init();

    // Set up cancellation and task tracking
    let cancel_token = CancellationToken::new();
    let tracker = TaskTracker::new();

    // Set up the database connection pool
    debug!("using db file at {:?}", &config.db_file);
    let cores = std::thread::available_parallelism()?.get() as u32;
    // This is a low-traffic service running on shared hardware, so go easy on parallelism.
    // Up to (cores - 2) threads, with a minimum of 2.
    let max_readers = cores.saturating_sub(2).max(2);
    debug!(
        "{} cores available, limiting db reader threads to {}",
        cores, max_readers
    );
    let read_pool = db_pool(&config.db_file, max_readers).await?;
    let write_pool = db_pool(&config.db_file, 1).await?;
    let db = Db::new(read_pool, write_pool, tracker.clone());

    // If we're in one of our "do migrations" modes instead of our normal mode,
    // do the deed now and exit early.
    if options.migrate {
        println!("--migrate: running database migrations now.");
        db.migrations().run().await?;
        println!("--migrate: finished migrations. see u, space cowboy.");

        db.close().await;
        return Ok(());
    } else if options.status {
        println!("Database migrations status:");
        let statuses = db.migrations().info().await?;
        for status in statuses.iter() {
            println!("{}", status);
        }

        db.close().await;
        return Ok(());
    }

    // We're in normal mode, but maybe check the migrations.
    if config.validate_migrations {
        info!("validating database migrations");
        db.migrations().validate().await?;
    }

    // Set up the cookie key
    let key = load_cookie_key(&config.key_file).await?;

    // Build the app state
    let templates = load_templates()?;
    let inner = DSInner {
        db: db.clone(),
        config,
        templates,
        cookie_key: key,
        task_tracker: tracker.clone(),
        cancel_token: cancel_token.clone(),
    };
    let state: DogState = Arc::new(inner);

    // ok, ok,...
    let app = eardogger_app(state.clone());

    // Spawn the shutdown signal listener, outside the tracker
    tokio::spawn(cancel_on_terminate(cancel_token.clone()));

    // Spawn the stale session pruning worker, in the tracker
    tracker.spawn(prune_stale_sessions_worker(
        db.clone(),
        cancel_token.clone(),
    ));

    // Serve the website til we're done!
    let serve_result = match state.config.mode {
        ServeMode::Http { port } => {
            info!("starting main HTTP server loop, serving on port {}", port);
            let listener = TcpListener::bind(("0.0.0.0", port)).await?;
            axum::serve(listener, app)
                .with_graceful_shutdown(cancel_token.clone().cancelled_owned())
                .await
        }
        ServeMode::Fcgi { max_connections } => {
            info!("starting main FastCGI server loop");
            busride_rs::serve_fcgid_with_graceful_shutdown(
                app,
                max_connections,
                cancel_token.clone().cancelled_owned(),
            )
            .await
        }
    };

    // Clean up:
    if let Err(e) = serve_result {
        // It's possible there was no cancel signal sent earlier, so send one now.
        error!("server loop exited with an error: {}", e);
        cancel_token.cancel();
    }
    info!("waiting for tasks to finish");
    tracker.close();
    tracker.wait().await;
    db.close().await;
    info!("see ya!");

    Ok(())
}

/// Either load the cookie key from a binary file, or create one.
async fn load_cookie_key(path: impl AsRef<Path>) -> tokio::io::Result<Key> {
    let path = path.as_ref();
    if fs::try_exists(path).await? {
        debug!("loading existing cookie keyfile at {:?}", path);
        let mut f = File::open(path).await?;
        let mut keybuf = [0u8; 64];
        f.read_exact(&mut keybuf).await?;
        let key = Key::from(&keybuf);
        Ok(key)
    } else {
        debug!("generating new cookie keyfile at {:?}", path);
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
#[tracing::instrument(skip_all)]
async fn cancel_on_terminate(cancel_token: CancellationToken) {
    use tokio::signal::{
        ctrl_c,
        unix::{signal, SignalKind},
    };
    let Ok(mut terminate) = signal(SignalKind::terminate()) else {
        // If we can't listen for the signal, bail immediately
        error!("couldn't even establish SIGTERM signal listener; taking my ball and going home");
        cancel_token.cancel();
        return;
    };
    // Wait indefinitely until we hear a shutdown signal.
    // The ctrl_c function listens for SIGINT, the other one listens for SIGTERM
    // (aka `kill`/`killall` with no flags).
    select! {
        _ = ctrl_c() => {
            // don't care if Ok or Err
            info!("received SIGINT, starting shutdown");
        },
        _ = terminate.recv() => {
            // don't care if Some or None
            info!("received SIGTERM, starting shutdown");
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
#[tracing::instrument(skip_all)]
async fn prune_stale_sessions_worker(db: Db, cancel_token: CancellationToken) {
    info!("starting up session pruning worker; pausing before first purge");
    let a_day = Duration::from_secs(60 * 60 * 24);
    // Initial delay (or fast-track it on cancel)
    select! {
        _ = tokio::time::sleep(Duration::from_secs(10)) => {},
        _ = cancel_token.cancelled() => {},
    }
    loop {
        info!("purging stale sessions...");
        match db.sessions().delete_expired().await {
            Ok(count) => {
                info!("purged {} sessions, going back to sleep", count);
            }
            Err(e) => {
                error!(
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
    info!("shutting down session pruning worker");
}
