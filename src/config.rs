use serde::Deserialize;
use std::{
    num::NonZeroUsize,
    path::{Path, PathBuf},
};
use thiserror::Error;
use url::Url;

#[derive(Error, Debug)]
pub enum ConfError {
    // The generated code for returning an error is cheaper than maybe panicking.
    #[error("a prior check guaranteed that this error would never happen.")]
    Impossible,
}

#[derive(Debug, Deserialize, Clone)]
pub enum ServeMode {
    #[serde(alias = "http")]
    Http { port: u16 },
    #[serde(alias = "fcgi")]
    Fcgi { max_connections: NonZeroUsize },
}

/// Stuff the app needs that's sourced from configuration.
#[derive(Clone, Debug)]
pub struct DogConfig {
    /// Whether we're running in production or not. Currently not really consulted
    /// for anything.
    pub production: bool,
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
}

/// The intermediate struct used for deserializing the config file and
/// generating a usable DogConfig struct.
#[derive(Debug, Deserialize)]
struct PreDogConfig {
    production: bool,
    mode: ServeMode,
    validate_migrations: bool,
    public_url: String,
    // These three file paths can be absolute, or relative to the config file's dir.
    db_file: String,
    assets_dir: String,
    key_file: String,
}

impl PreDogConfig {
    fn finalize(self, base_dir: &Path) -> anyhow::Result<DogConfig> {
        // Parse the URL (only fallible bit for now)
        let public_url = Url::parse(&self.public_url)?;
        // Join the file paths
        let db_file = base_dir.join(&self.db_file);
        let assets_dir = base_dir.join(&self.assets_dir);
        let key_file = base_dir.join(&self.key_file);
        // Chomp the rest
        let Self {
            production,
            mode,
            validate_migrations,
            ..
        } = self;
        Ok(DogConfig {
            production,
            mode,
            validate_migrations,
            public_url,
            db_file,
            assets_dir,
            key_file,
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
    pub fn temp_test() -> anyhow::Result<Self> {
        let pre = PreDogConfig {
            production: false,
            mode: ServeMode::Http { port: 443 },
            validate_migrations: false,
            public_url: "http://eardogger.com".to_string(),
            // tests build their own in-memory db pools anyway.
            db_file: "ignore_me".to_string(),
            assets_dir: "public".to_string(),
            key_file: "cookie_key.bin".to_string(),
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
