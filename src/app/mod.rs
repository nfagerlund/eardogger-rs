mod authentication;
mod routes;
pub mod state;
mod templates;
mod web_result;

use authentication::{session_middleware, token_middleware};
use routes::*;
use state::DogState;
pub use templates::load_templates;

use axum::{
    handler::HandlerWithoutStateExt,
    middleware::from_fn_with_state,
    routing::{delete, get, post},
    Router,
};
use tower_cookies::CookieManagerLayer;
use tower_http::services::ServeDir;

/// Return a fully-functional eardogger app! The caller is in charge of building
/// the state, but we DO need it here in order to construct our auth middleware,
/// since we're using slacker mode instead of writing proper Tower middleware types.
pub fn eardogger_app(state: DogState) -> Router {
    let session_auth = from_fn_with_state(state.clone(), session_middleware);
    let token_auth = from_fn_with_state(state.clone(), token_middleware);
    Router::new()
        .route("/", get(root))
        .route("/mark/:url", get(mark_url))
        .route("/mark", post(post_mark))
        .route("/resume/:url", get(resume))
        .route("/faq", get(faq))
        .route("/account", get(account))
        .route("/install", get(install))
        .route("/login", post(post_login))
        .route("/logout", post(post_logout))
        .route("/signup", post(post_signup))
        .route("/changepassword", post(post_changepassword))
        .route("/fragments/dogears", get(fragment_dogears))
        .route("/fragments/tokens", get(fragment_tokens))
        .route("/fragments/personalmark", post(post_fragment_personalmark))
        .route("/tokens/:id", delete(delete_token))
        .route("/api/v1/list", get(api_list))
        .route("/api/v1/dogear/:id", delete(api_delete))
        .route("/api/v1/create", post(api_create))
        .route(
            "/api/v1/update",
            post(api_update).options(api_update_cors_preflight),
        )
        .layer(token_auth) // inner, so can override session.
        .layer(session_auth)
        .layer(CookieManagerLayer::new())
        // put static files and 404 outside the auth layers
        .nest_service(
            "/public",
            ServeDir::new(&state.config.assets_dir).not_found_service(four_oh_four.into_service()),
        )
        .route("/status", get(status))
        .fallback(four_oh_four)
        .with_state(state)
}
