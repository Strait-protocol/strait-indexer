# Running Strait on Supabase

Strait stores indexed data in Postgres via `sqlx`. Supabase is managed Postgres, so it works as the backing store with no schema changes — only connection configuration differs.

## 1. Create the project

In the [Supabase dashboard](https://supabase.com/dashboard), create a project and note the database password you set.

## 2. Get the connection string

Project Settings → **Database** → **Connection string** → **URI**. Supabase offers three endpoints:

| Endpoint | Port | Use for |
|---|---|---|
| **Session pooler** | 5432 | The running indexer (recommended) — supports prepared statements |
| **Transaction pooler** | 6543 | Serverless / many short-lived connections |
| **Direct connection** | 5432 (`db.<ref>.supabase.co`) | Applying migrations; IPv6-only on some plans |

Set `DATABASE_URL` in your `.env`. Always include `?sslmode=require` — Supabase rejects non-SSL connections:

```
DATABASE_URL=postgres://postgres.<project-ref>:<password>@aws-0-<region>.pooler.supabase.com:5432/postgres?sslmode=require
```

## 3. Apply the migrations

Migrations live in [`crates/strait-store/migrations/`](../crates/strait-store/migrations/) and run automatically on node startup via `Database::migrate()`. To apply them manually first:

```bash
# Point sqlx-cli at the direct connection for DDL
export DATABASE_URL='postgres://postgres:<password>@db.<project-ref>.supabase.co:5432/postgres?sslmode=require'
cargo install sqlx-cli --no-default-features --features rustls,postgres
sqlx migrate run --source crates/strait-store/migrations
```

The schema uses the `uuid-ossp` extension (`CREATE EXTENSION IF NOT EXISTS "uuid-ossp"`), which is pre-available on Supabase — no extra setup needed.

## What Strait handles automatically

The connection layer in [`crates/strait-store/src/db.rs`](../crates/strait-store/src/db.rs) adapts to Supabase:

- **TLS** is enabled by default (SSL mode upgraded to `Require`) unless the URL has `sslmode=disable`.
- **Transaction pooler** (port `6543` or a `pooler.supabase.com` host) automatically disables the prepared-statement cache, which PgBouncer in transaction mode does not support.

So switching from local Postgres to Supabase is purely a `DATABASE_URL` change.

## Local development

For local Postgres, disable SSL explicitly:

```
DATABASE_URL=postgres://postgres:password@localhost:5432/strait?sslmode=disable
```
