use serde::Deserialize;
use std::{
    num::NonZeroUsize,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};
use thiserror::Error;
use url::Url;

static IS_PRODUCTION: AtomicBool = AtomicBool::new(false);

/// Whether the app is running in production or not. This is mostly relevant
/// when deciding whether to expose the details of a 500 error. Unfortunately,
/// the spot where we need to _know_ it doesn't have access to a DogConfig,
/// so we stash the value in a global var when loading the config (which only
/// happens once) and let you read it from here.
pub fn is_production() -> bool {
    IS_PRODUCTION.load(Ordering::Relaxed)
}

#[derive(Error, Debug)]
pub enum ConfError {
    // The generated code for returning an error is cheaper than maybe panicking.
    #[error("a prior check guaranteed that this error would never happen.")]
    Impossible,
}

/// Settings for running the app server.
#[derive(Debug, Deserialize, Clone)]
pub enum ServeMode {
    #[serde(alias = "http")]
    Http { port: u16 },
    #[serde(alias = "fcgi")]
    Fcgi { max_connections: NonZeroUsize },
}

/// Settings for logging
#[derive(Debug, Deserialize, Clone)]
pub struct LogConfig {
    /// A [`tracing_subscriber::EnvFilter`] string.
    pub filter: String,
    /// Whether to log to stdout. Always set false under fcgi, because
    /// mod_fcgid spams the server's global ErrorLog.
    pub stdout: bool,
    /// Whether to log to an auto-rotating log file.
    pub file: Option<LogFileConfig>,
}

/// Settings for logging to an auto-rotating log file.
#[derive(Debug, Deserialize, Clone)]
pub struct LogFileConfig {
    /// The directory to use for log files.
    pub directory: PathBuf,
    /// The log file prefix. Files will have names like `<name>.<timestamp>.log`.
    pub name: String,
    /// How many days of logs to keep. Excess logs are auto-deleted.
    pub days: usize,
}

/// Stuff the app needs that's sourced from configuration.
#[derive(Clone, Debug)]
pub struct DogConfig {
    /// Whether we're running in production or not. Masks 500 error details when true.
    pub production: bool,
    /// How many OS worker threads the Tokio runtime will use. By default,
    /// tokio will use "the number of cores available to the system," which
    /// *it's possible* your web host will hate. Must be > 0.
    pub runtime_threads: usize,
    /// How many DB reader threads to cap out at. Must be > 0. This is _in addition_
    /// to the Tokio runtime threads. Also, there's always one additional thread for
    /// the DB writer. For best results, ensure (runtime_threads + reader_threads + 1)
    /// is ≤ the number of virtual CPU cores in your system.
    pub reader_threads: u32,
    /// Whether to serve in FastCGI or HTTP mode, with mode-specific settings embedded.
    pub mode: ServeMode,
    /// Whether to check the integrity of database migrations before continuing
    /// startup.
    pub validate_migrations: bool,
    /// The site's own public-facing base URL.
    pub public_url: Url,
    /// The location of the database file.
    pub db_file: PathBuf,
    /// The directory with static CSS/JS/image assets.
    pub assets_dir: PathBuf,
    /// Location of the binary key file for signing cookies. We'll auto-create this if it
    /// doesn't exist already.
    pub key_file: PathBuf,
    /// Settings for application logging via Tracing subscriber layers.
    pub log: LogConfig,
}

/// The intermediate struct used for deserializing the config file and
/// generating a usable DogConfig struct.
#[derive(Debug, Deserialize)]
struct PreDogConfig {
    production: bool,
    runtime_threads: usize,
    reader_threads: u32,
    mode: ServeMode,
    validate_migrations: bool,
    public_url: String,
    // These three file paths can be absolute, or relative to the config file's dir.
    db_file: String,
    assets_dir: String,
    key_file: String,
    log: LogConfig,
}

impl PreDogConfig {
    fn finalize(self, base_dir: &Path) -> anyhow::Result<DogConfig> {
        // Destructure yourself
        let Self {
            production,
            runtime_threads,
            reader_threads,
            mode,
            validate_migrations,
            public_url,
            db_file,
            assets_dir,
            key_file,
            mut log,
        } = self;

        // Publish IS_PRODUCTION
        IS_PRODUCTION.store(production, Ordering::Relaxed);
        // Parse the URL (only fallible bit for now)
        let public_url = Url::parse(&public_url)?;
        // Join the file paths
        let db_file = base_dir.join(db_file);
        let assets_dir = base_dir.join(assets_dir);
        let key_file = base_dir.join(key_file);
        if let Some(logfile) = &mut log.file {
            logfile.directory = base_dir.join(&logfile.directory);
        }
        Ok(DogConfig {
            production,
            runtime_threads,
            reader_threads,
            mode,
            validate_migrations,
            public_url,
            db_file,
            assets_dir,
            key_file,
            log,
        })
    }
}

impl DogConfig {
    /// Load app configuration from a config file. The provided path can be absolute
    /// or relative to the current working directory.
    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let cwd = std::env::current_dir()?;
        let abs_path = cwd.join(path.as_ref());
        // This runs before we have a tracing subscriber, so we have to log rudely.
        println!("Startup: loading config file from {:?}", &abs_path);
        let base_dir = abs_path.parent().ok_or(ConfError::Impossible)?;
        let conf_text = std::fs::read_to_string(&abs_path)?;
        let pre: PreDogConfig = toml::from_str(&conf_text)?;
        pre.finalize(base_dir)
    }

    #[cfg(test)]
    pub fn test_config() -> anyhow::Result<Self> {
        // Ignoring the one writer thread...
        let max_threads = usize::from(std::thread::available_parallelism()?).clamp(2, 10);

        let pre = PreDogConfig {
            production: false,
            runtime_threads: max_threads / 2,
            reader_threads: (max_threads as u32) / 2,
            mode: ServeMode::Http { port: 443 },
            validate_migrations: false,
            public_url: "http://eardogger.com".to_string(),
            // tests build their own in-memory db pools anyway.
            db_file: "ignore_me".to_string(),
            assets_dir: "public".to_string(),
            key_file: "cookie_key.bin".to_string(),
            log: LogConfig {
                filter: "info".to_string(),
                stdout: true,
                file: None,
            },
        };
        let cwd = std::env::current_dir()?;
        pre.finalize(&cwd)
    }
}

#[cfg(test)]
#[test]
fn valid_example_config_file() {
    DogConfig::load("eardogger.example.toml")
        .expect("example config file is valid and up-to-date with impl");
}
