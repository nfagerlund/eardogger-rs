CREATE TABLE IF NOT EXISTS dogears(
    id INTEGER PRIMARY KEY NOT NULL,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    prefix TEXT NOT NULL,
    current TEXT NOT NULL,
    display_name TEXT,
    updated TIMESTAMP NOT NULL DEFAULT current_timestamp,
    UNIQUE (user_id, prefix) ON CONFLICT ROLLBACK
);
