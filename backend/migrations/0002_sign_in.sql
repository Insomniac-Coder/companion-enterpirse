-- Phase 1, task 3: who is calling. People sign in through the company's identity provider
-- (OpenID Connect: Microsoft Entra ID or any standard one); software uses API keys; an install
-- without sign-in (a laptop, reachable only from itself) runs as one local person.
-- Times here are TIMESTAMPTZ, set and compared by the database itself.

CREATE TABLE users (
    id TEXT PRIMARY KEY,
    -- The identity provider and its id for the person; ('local', 'local') for the local person.
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    email TEXT NOT NULL DEFAULT '',
    name TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (issuer, subject)
);

INSERT INTO users (id, issuer, subject, name) VALUES ('local', 'local', 'local', 'You');

-- A signed-in browser. The cookie holds a random value; only its SHA-256 is kept here, so a copy
-- of the database cannot be used to sign in.
CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX sessions_user ON sessions (user_id);

-- Keys for software acting for a person. Shown once when made; only the SHA-256 is kept.
CREATE TABLE api_keys (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    key_hash TEXT NOT NULL UNIQUE,
    -- The key's first characters, to tell keys apart in a list.
    hint TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ
);
CREATE INDEX api_keys_user ON api_keys (user_id);

-- A sign-in in progress: what the identity provider must send back, used once.
CREATE TABLE sign_in_attempts (
    state TEXT PRIMARY KEY,
    nonce TEXT NOT NULL,
    pkce_verifier TEXT NOT NULL,
    return_to TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
