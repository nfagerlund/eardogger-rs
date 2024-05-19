-- This migration logs everyone out as a side-effect.
DROP TABLE sessions;

CREATE TABLE IF NOT EXISTS sessions(
    external_id INTEGER PRIMARY KEY NOT NULL,
    id TEXT UNIQUE NOT NULL,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    csrf_token TEXT NOT NULL,
    expires TIMESTAMP NOT NULL,
    user_agent TEXT
);
