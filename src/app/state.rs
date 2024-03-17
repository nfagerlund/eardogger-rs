use crate::db::Db;

/// Stuff for the stuff gods!!!
#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub config: AppConfig,
}

/// Stuff that should be sourced from configuration, but for right now
/// I'm just cramming it in wherever.
#[derive(Clone)]
pub struct AppConfig {
    pub is_prod: bool,
}
