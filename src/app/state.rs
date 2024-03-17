use crate::db::Db;
use std::sync::Arc;

pub type DogState = Arc<DSInner>;

/// Stuff for the stuff gods!!!
#[derive(Clone)]
pub struct DSInner {
    pub db: Db,
    pub config: DogConfig,
}

/// Stuff that should be sourced from configuration, but for right now
/// I'm just cramming it in wherever.
#[derive(Clone)]
pub struct DogConfig {
    pub is_prod: bool,
}
