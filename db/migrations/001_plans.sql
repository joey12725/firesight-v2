-- FIRESight's own tables. Better Auth's tables ("user", "session", "apikey", "oauthClient", …)
-- are created by its schema migration, which scripts/migrate.ts runs before this file.

-- One data key per user, wrapped by the server key (PLAN_KEK). Deleting the user deletes
-- the row, after which no copy of that user's plan ciphertext can be read again.
create table if not exists user_key (
  user_id     text primary key references "user" (id) on delete cascade,
  dek_wrapped bytea not null,
  kek_version integer not null,
  created_at  timestamptz not null default now()
);

-- A synced plan. The name and the document are AES-256-GCM ciphertext under the user's
-- data key; the rest is what the server needs to list, order and guard writes.
create table if not exists plan (
  id         text primary key,
  user_id    text not null references "user" (id) on delete cascade,
  name_ct    bytea not null,
  doc_ct     bytea not null,
  -- Size of the plan as UTF-8 JSON, before encryption.
  bytes      integer not null,
  -- Starts at 1 and goes up by one on every write. Writers send the rev they read.
  rev        integer not null default 1,
  -- 'web', 'apikey:<key name>' or 'oauth:<client name>'.
  updated_by text not null,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);

create index if not exists plan_user_updated_idx on plan (user_id, updated_at desc);

-- Fixed-window request counters, one row per key, shared by every function instance.
create table if not exists firesight_rate (
  key          text primary key,
  window_start bigint not null,
  count        integer not null
);
