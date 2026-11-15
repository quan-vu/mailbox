# ADR-0003 — SQLite Desktop; PostgreSQL Web

Date: 2026-11-14 • Status: Accepted target design (not wired)

**Decision**: Use SQLite for user-owned Desktop metadata, PostgreSQL for server-hosted Web. Repository ports and contract tests make domain behavior shared. Credentials in OS keychain Desktop or separately encrypted Web vault, not in either plain DB.

**Consequences**: Two SQL migration families and repository tests; avoid dialect-specific SQL leakage into business use cases. For prototype no runtime storage is currently configured.
