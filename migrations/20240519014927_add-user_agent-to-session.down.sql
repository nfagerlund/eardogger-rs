DROP TABLE sessions;

-- re-create the old schema

CREATE TABLE IF NOT EXISTS sessions(
    id TEXT PRIMARY KEY NOT NULL,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    csrf_token TEXT NOT NULL,
    expires TIMESTAMP NOT NULL
);
