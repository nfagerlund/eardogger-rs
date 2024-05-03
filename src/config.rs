use serde::Deserialize;
use std::path::{Path, PathBuf};
use url::Url;

/// Stuff the app needs that's sourced from configuration.
#[derive(Clone, Debug)]
pub struct DogConfig {
    /// Whether we're running in production or not. Currently not really consulted
    /// for anything.
    pub production: bool,
    /// The site's own public-facing base URL.
    pub public_url: Url,
    /// The port to listen on, if running our own http server.
    pub port: u16,
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
    public_url: String,
    port: u16,
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
            production, port, ..
        } = self;
        Ok(DogConfig {
            production,
            public_url,
            port,
            db_file,
            assets_dir,
            key_file,
        })
    }
}

impl DogConfig {
    // TODO: replace all this
    pub fn temp_dev() -> anyhow::Result<Self> {
        let loaded = std::fs::read_to_string("eardogger.toml")?;
        let pre: PreDogConfig = toml::from_str(&loaded)?;
        let cwd = std::env::current_dir()?;
        pre.finalize(&cwd)
    }

    #[cfg(test)]
    pub fn temp_test() -> anyhow::Result<Self> {
        let pre = PreDogConfig {
            production: false,
            public_url: "http://eardogger.com".to_string(),
            port: 443,
            // tests build their own in-memory db pools anyway.
            db_file: "ignore_me".to_string(),
            assets_dir: "public".to_string(),
            key_file: "cookie_key.bin".to_string(),
        };
        let cwd = std::env::current_dir()?;
        pre.finalize(&cwd)
    }
}
