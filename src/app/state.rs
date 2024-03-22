use serde::Serialize;
use std::sync::Arc;
use tower_cookies::Key;
use url::Url;

use crate::db::Db;

pub type DogState = Arc<DSInner>;

/// Stuff for the stuff gods!!!
#[derive(Clone)]
pub struct DSInner {
    pub db: Db,
    pub config: DogConfig,
    pub templates: minijinja::Environment<'static>,
    pub cookie_key: Key,
}

/// Stuff that should be sourced from configuration, but for right now
/// I'm just cramming it in wherever.
#[derive(Clone)]
pub struct DogConfig {
    pub is_prod: bool,
    /// The site's own base URL.
    pub own_origin: Url,
}

impl DSInner {
    fn render_view<S: Serialize>(&self, name: &str, ctx: S) -> Result<String, minijinja::Error> {
        self.templates.get_template(name)?.render(ctx)
    }
}
