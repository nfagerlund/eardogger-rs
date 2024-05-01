use url::Url;

/// Stuff that should be sourced from configuration, but for right now
/// I'm just cramming it in wherever.
#[derive(Clone, Debug)]
pub struct DogConfig {
    pub is_prod: bool,
    /// The site's own base URL.
    pub own_url: Url,
    /// The port to listen on, if running our own http server.
    pub port: u16,
    /// The directory with static CSS/JS/image assets.
    pub assets_dir: String,
}

impl DogConfig {
    // TODO: replace all this
    pub fn temp_dev() -> anyhow::Result<Self> {
        let own_url = Url::parse("http://localhost:3000")?;
        let assets_dir = "public".to_string();
        Ok(Self {
            is_prod: false,
            own_url,
            port: 3000,
            assets_dir,
        })
    }

    #[cfg(test)]
    pub fn temp_test() -> anyhow::Result<Self> {
        let own_url = Url::parse("http://eardogger.com")?;
        let assets_dir = "public".to_string();
        Ok(Self {
            is_prod: false,
            own_url,
            port: 443,
            assets_dir,
        })
    }
}
