//! All right, so: sqlite has some unusual characteristics around concurrency that
//! become pretty important to keep track of whenever you're doing server-side logic
//! in a multi-threaded runtime. This is one of the primary influences on the data
//! layer design for this iteration of Eardogger, and it's going to remain relevant
//! for any future stuff I build using this same toolkit. So I'm writing it down
//! here, which is hopefully where my future self will search first.
//!
//! First off, this article was invaluable for helping solidify my understanding of
//! the constraints and their appropriate workarounds:
//! <https://kerkour.com/sqlite-for-servers> ...and never mind that he personally
//! decided a month later to fuck off back to Postgres; that's a perfectly
//! reasonable take, but I'm still pretty sure the operational characteristics of
//! sqlite make it a winner for small and unpopular services with extremely long
//! lifetimes.
//!
//! Anyway!
//!
//! A sqlite database on disk can be accessed by multiple processes and/or threads
//! at once, and it uses filesystem-based locking and signalling tools to coordinate
//! concurrent access. Under normal operation in WAL mode, a single database can
//! concurrently support:
//!
//! - Any number of readers, AND
//! - Up to one writer
//! - (Also, there's certain maintenance operations that require totally exclusive
//!   locks, like WAL checkpointing. Never mind those for now.)
//!
//! If someone already holds a write lock and you try to do a write, the database is
//! considered _busy,_ and there's a couple of possible behaviors that can result:
//!
//! - If you tried to start an _immediate_ write transaction, the sqlite C function
//!   you called to do it will go into a busy loop for _up to_ the duration of your
//!   configured "busy timeout", repeatedly attempting to get a write lock.
//!     - Executing a one-off `INSERT/UPDATE/DELETE` etc. statement without a
//!       `BEGIN` block is considered an immediate write transaction. Yay!
//!     - So are `BEGIN IMMEDIATE` transaction blocks.
//! - If you were in the middle of a _deferred_ transaction (plain `BEGIN` block)
//!   that currently holds a read lock and attempt a statement that would _upgrade it_
//!   to holding a write lock, the sqlite function you called will immediately return
//!   with a `SQLITE_BUSY` error code. So basically you'll need to be ready to catch
//!   busy errors and re-try transactions.
//!     - **Unfortunately,** sqlx's `pool.begin()` transaction feature can only
//!       start deferred transactions; the library currently has no way to start an
//!       immediate transaction with sqlite. This basically blows.
//!
//! The upshot is that pretty much no matter what, your sqlite-using application
//! code will need more logic around coordinating global database concurrency than
//! would be necessary for a smart server-based DB that supports row-level locking
//! semantics. Them's the breaks!!
//!
//! I ran some experiments (if you're me, you can find the repo in the junk drawer
//! under `rust-chaos-sqlite-concurrency`), and basically what I found out is:
//!
//! - One-off write statements are _almost always_ safe even with an unbounded pool
//!   of worker threads, but under high contention they can end up wasting a bunch of
//!   time in a disk-mediated spin-lock, and a too-low busy-timeout can still result
//!   in occasional errors.
//! - If you restrict your pool size so there's only ever one writer thread
//!   available, it prevents nearly all problems. Deferred transactions are fine, even
//!   under heavy load, and I think even one-off writes can end up performing more
//!   reliably because it offloads the waiting onto the async runtime's semaphore
//!   functionality instead of a spin-lock. However:
//!     - You need to remember to correctly use the single-writer pool for any
//!       operation that might need a write lock.
//!     - This scheme only works within a single process — if there's another
//!       process somewhere concurrently accessing the same database file, all bets
//!       are off and you can still get busy errors in transactions.
//!
//! So the design I ended up going for in this app is:
//!
//! - A `Db` type that contains a single-connection write pool, a many-connection
//!   read pool, and a handle for spawning async tasks that you don't want interrupted
//!   by shutdown.
//! - A bunch of pluralized query helper types (`Users`, etc.) that borrow a
//!   reference to a `Db`, and methods on `Db` for handing out said helpers.
//! - The query helper methods are in charge of making sure they use the correct
//!   pool for any given operation. Writes go to the writer, reads go to the readers.
//! - Query helper methods should keep an eye out for side-effect writes that might
//!   not be on the critical path for their return value, with the canonical example
//!   being "last used" timestamps. These kinds of incidental writes can be offloaded
//!   to a spawned task, so we can return the useful part of the query without having
//!   to await a connection from the write pool.

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
