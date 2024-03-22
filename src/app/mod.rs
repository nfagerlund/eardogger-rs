mod authentication;
mod routes;
pub mod state;
mod templates;
mod web_result;

use authentication::session_middleware;
use state::DogState;
pub use templates::load_templates;

use axum::{
    middleware::from_fn_with_state,
    routing::{get, post},
    Router,
};
use tower_cookies::CookieManagerLayer;
use tower_http::services::ServeDir;

/// Return a fully-functional eardogger app! The caller is in charge of building
/// the state, but we DO need it here in order to construct our auth middleware,
/// since we're using slacker mode instead of writing proper Tower middleware types.
pub fn eardogger_app(state: DogState) -> Router {
    let session_auth = from_fn_with_state(state.clone(), session_middleware);
    Router::new()
        .route("/", get(routes::root))
        .route("/login", post(routes::post_login))
        .layer(session_auth)
        .layer(CookieManagerLayer::new())
        // put static files outside the auth layers
        .nest_service("/public", ServeDir::new(&state.config.assets_dir))
        .with_state(state)
}
