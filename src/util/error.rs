use http::StatusCode;
use thiserror::Error;

/// A conversion helper trait that lets specific error types transform
/// into both `WebError`s and `ApiError`s if they get "?"-propagated out of
/// handler fns. This feeds the blanket From impls for the AppError variants.
pub trait IntoHandlerError {
    /// Consume self and return an http::StatusCode and an owned String message.
    fn status_and_message(self) -> (StatusCode, String);
}

/// Errors that are the user's fault (and which they therefore might be able
/// to do something about).
#[derive(Debug, Error)]
pub enum UserError {
    #[error("dogear not found")]
    Dogear404,

    #[error("The provided URL ({url}) doesn't match the provided prefix ({prefix})")]
    DogearNonMatching { url: String, prefix: String },

    #[error("You already have a bookmark with that prefix: {prefix}")]
    DogearExists { prefix: String },

    #[error("Can't bookmark an invalid or non-http(s) URL: {url}")]
    DogearInvalidUrl { url: String },

    // This happens when the provided value of the Origin header can't
    // be turned back into a HeaderValue. I'm pretty sure something
    // further out in the stack would explode long before this reached my code.
    #[error("That HTTP request was mangled in a way most would believe impossible. Good work, but also I can't do anything with it.")]
    HttpFucked,

    #[error("Something impossible happened: {0}")]
    Impossible(&'static str),

    #[error("Requested page size is too large")]
    PageOversize,
}

impl IntoHandlerError for UserError {
    fn status_and_message(self) -> (StatusCode, String) {
        let status = match &self {
            UserError::Dogear404 => StatusCode::NOT_FOUND,
            UserError::DogearNonMatching { .. } => StatusCode::BAD_REQUEST,
            UserError::DogearExists { .. } => StatusCode::CONFLICT,
            UserError::DogearInvalidUrl { .. } => StatusCode::BAD_REQUEST,
            UserError::HttpFucked => StatusCode::IM_A_TEAPOT,
            UserError::Impossible(_) => StatusCode::INTERNAL_SERVER_ERROR,
            UserError::PageOversize => StatusCode::BAD_REQUEST,
        };
        (status, self.to_string())
    }
}

// Blanket impl for turning an anyhow into a 500 error.
impl IntoHandlerError for anyhow::Error {
    fn status_and_message(self) -> (http::StatusCode, String) {
        // For quick-and-dirty error returns, use a default HTTP error code of 500.
        // This is almost always correct.
        (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
    }
}

// Template errors are also 500s.
impl IntoHandlerError for minijinja::Error {
    fn status_and_message(self) -> (StatusCode, String) {
        (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
    }
}

// Db errors are 500s unless we're using a mixed result to turn
// some of them into 4xxs instead (like with unique conflicts).
impl IntoHandlerError for sqlx::Error {
    fn status_and_message(self) -> (StatusCode, String) {
        (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
    }
}

/// Helper enum for when a given function might return errors from
/// two separate domains (server errors that should be masked, and user
/// errors that should be elaborated).
#[derive(Debug, Error)]
pub enum MixedError<T>
where
    T: IntoHandlerError,
{
    #[error("{0}")]
    User(UserError),
    #[error("{0}")]
    Server(T),
}

impl<T> IntoHandlerError for MixedError<T>
where
    T: IntoHandlerError,
{
    fn status_and_message(self) -> (StatusCode, String) {
        match self {
            MixedError::User(e) => e.status_and_message(),
            MixedError::Server(e) => e.status_and_message(),
        }
    }
}

impl<T> From<UserError> for MixedError<T>
where
    T: IntoHandlerError,
{
    fn from(value: UserError) -> Self {
        Self::User(value)
    }
}
// Alas, we cannot blanket impl, for the same reason you can't have
// a blanket impl for T: Error overlapping with type-specific impls.
impl From<sqlx::Error> for MixedError<sqlx::Error> {
    fn from(value: sqlx::Error) -> Self {
        Self::Server(value)
    }
}
