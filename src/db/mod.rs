//! Ok, how about this. We make a Db type that wraps a pool, and a bunch
//! of pluralized collection types (Users, etc.) that just hold a reference
//! to a pool, and methods on Db that return those collections. So then it's
//! like `db.users().create(...)`. Seems ok.
mod core;
mod db_tests;
mod dogears;
mod sessions;
mod tokens;
mod users;

// Publicize the record types, they're the star of the show
pub use self::dogears::Dogear;
pub use self::sessions::Session;
pub use self::tokens::{Token, TokenScope};
pub use self::users::User;

// And the main wrapper type
pub use self::core::Db;
