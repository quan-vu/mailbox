# ADR 0005 — One repository implementation over two SQL dialects

- Status: Accepted (2026-11-14, MB-120)
- Builds on: ADR 0003 (SQLite on Desktop, PostgreSQL on Web)

## Context

ADR 0003 keeps two databases. Writing every query twice would let the Desktop and Web stores drift, and the starter migrations used dialect-specific types (UUID, TIMESTAMPTZ, BOOLEAN on PostgreSQL; TEXT and INTEGER on SQLite) that force separate code paths.

## Decision

- `crates/mailbox-store` implements `mailbox_core::store::MailStore` once (a macro instantiates it for `SqliteStore` and `PgStore`) with `$n` placeholders, which both drivers accept.
- The columns the repository reads and writes use portable types in both dialects: ids are opaque `TEXT` (the application generates them), times are Unix seconds in UTC, flags are `0/1` integers with a `CHECK`. PostgreSQL uses `BIGINT` where SQLite uses `INTEGER`.
- Each dialect keeps its own migration directory (`migrations/sqlite`, `migrations/postgres`), applied by the store on open/connect.
- One behavioural contract test suite (`crates/mailbox-store/tests/contract.rs`) runs against both.
- Every method takes `owner_id`. Rows of another owner are reported as `NotFound`, never as "forbidden", so ids cannot be probed.
- Storage errors leave the crate as short static codes only; SQL text and row data are never part of an error.

## Consequences

- Less idiomatic PostgreSQL (no native UUID/TIMESTAMPTZ/BOOLEAN). Accepted: correctness parity matters more here, and reporting views can cast.
- Dialect-specific features (full-text search, partial indexes) will need per-dialect queries later; they get their own ADR when introduced.
- Timestamps carry no time zone; presentation converts from UTC.
- The 0001 migrations were rewritten before any release, so no upgrade path from the starter's reference schema is needed. From here on migrations are append-only.
