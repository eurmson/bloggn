CREATE TABLE passkeys (
    id TEXT PRIMARY KEY NOT NULL,
    username TEXT NOT NULL,
    passkey TEXT NOT NULL,
    authorized BOOLEAN NOT NULL DEFAULT 0
);
