use url::Url;

/// Stuff that should be sourced from configuration, but for right now
/// I'm just cramming it in wherever.
#[derive(Clone, Debug)]
pub struct DogConfig {
    /// Whether we're running in production or not. Currently not really consulted
    /// for anything.
    pub is_prod: bool,
    /// The site's own public-facing base URL.
    pub public_url: Url,
    /// The port to listen on, if running our own http server.
    pub port: u16,
    /// The location of the database file.
    pub db_file: String,
    /// The directory with static CSS/JS/image assets.
    pub assets_dir: String,
    /// Location of the binary key file for signing cookies. We'll auto-create this if it
    /// doesn't exist already.
    pub key_file: String,
}

impl DogConfig {
    // TODO: replace all this
    pub fn temp_dev() -> anyhow::Result<Self> {
        let own_url = Url::parse("http://localhost:3000")?;
        let assets_dir = "public".to_string();
        Ok(Self {
            is_prod: false,
            public_url: own_url,
            port: 3000,
            db_file: "dev.db".to_string(),
            assets_dir,
            key_file: "cookie_key.bin".to_string(),
        })
    }

    #[cfg(test)]
    pub fn temp_test() -> anyhow::Result<Self> {
        let own_url = Url::parse("http://eardogger.com")?;
        let assets_dir = "public".to_string();
        Ok(Self {
            is_prod: false,
            public_url: own_url,
            port: 443,
            // tests build their own in-memory db pools anyway.
            db_file: "ignore_me".to_string(),
            assets_dir,
            key_file: "cookie_key.bin".to_string(),
        })
    }
}
