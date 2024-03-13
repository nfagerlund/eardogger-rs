-- Welcome to the rewrite of Eardogger! I'm starting this port with
-- the user and auth-related tables.

CREATE TABLE IF NOT EXISTS users(
    id INTEGER PRIMARY KEY NOT NULL,
    username TEXT UNIQUE NOT NULL,
    password_hash TEXT NOT NULL,
    email TEXT,
    created TIMESTAMP NOT NULL DEFAULT current_timestamp
);

-- Notes on users:
-- * username and password validity rules belong in application code,
--   not the schema. Sqlite ain't postgres, and this is not its forte.
--   (that said, I DID discover some cool tricks with NOT GLOB.)
-- * password_hash algorithm is bcrypt, and I don't currently intend to
--   add another kind... If I ever do, just inspect the hash format. The
--   newer ones use a standardized layout, and bcrypt is recognizable.

CREATE TABLE IF NOT EXISTS tokens(
    id INTEGER PRIMARY KEY NOT NULL,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    token_hash TEXT UNIQUE NOT NULL,
    scope TEXT NOT NULL,
    created TIMESTAMP NOT NULL DEFAULT current_timestamp,
    comment TEXT,
    last_used TIMESTAMP
);

-- Notes on tokens:
-- * scope is properly an enum, but sqlite doesn't party like that. So
--   make sure the enum in app code has an Invalid variant.

CREATE TABLE IF NOT EXISTS sessions(
    id TEXT PRIMARY KEY NOT NULL,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    csrf_token TEXT NOT NULL,
    expires TIMESTAMP NOT NULL
);
