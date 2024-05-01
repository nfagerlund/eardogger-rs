use url::Url;

/// Stuff that should be sourced from configuration, but for right now
/// I'm just cramming it in wherever.
#[derive(Clone, Debug)]
pub struct DogConfig {
    pub is_prod: bool,
    /// The site's own base URL.
    pub own_url: Url,
    /// The directory with static CSS/JS/image assets.
    pub assets_dir: String,
}
