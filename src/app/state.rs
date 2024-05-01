use serde::Serialize;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tower_cookies::Key;

use crate::config::DogConfig;
use crate::db::Db;
use crate::util::make_bookmarklet;

pub type DogState = Arc<DSInner>;

/// Stuff for the stuff gods!!!
#[derive(Clone, Debug)]
pub struct DSInner {
    pub db: Db,
    pub config: DogConfig,
    pub templates: minijinja::Environment<'static>,
    pub cookie_key: Key,
    pub task_tracker: TaskTracker,
    pub cancel_token: CancellationToken,
}

impl DSInner {
    #[tracing::instrument]
    pub fn render_view<S: Serialize + std::fmt::Debug>(
        &self,
        name: &str,
        ctx: S,
    ) -> Result<String, minijinja::Error> {
        self.templates.get_template(name)?.render(ctx)
    }

    /// Render a bookmarklet template into a `javascript:` URL.
    #[tracing::instrument]
    pub fn render_bookmarklet(
        &self,
        name: &str,
        token: Option<&str>,
    ) -> Result<String, minijinja::Error> {
        let ctx = minijinja::context! {
            own_origin => &self.config.own_url.origin().ascii_serialization(),
            token => token,
        };
        Ok(make_bookmarklet(
            &self.templates.get_template(name)?.render(ctx)?,
        ))
    }
}
