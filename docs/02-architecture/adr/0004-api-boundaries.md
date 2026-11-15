# ADR-0004 — Web API vs direct native adapters

Date: 2026-11-14 • Status: Accepted target design (not wired)

**Decision**: Browser UI calls authenticated Axum endpoints. Desktop native Rust use cases call IMAP/SMTP and local persistence through adapters **without opening arbitrary loopback ports**. Both share domain service ports and DTOs. In starter both platforms read deterministic fixture data.

**Consequences**: Separate UI data-provider implementations will be needed. Browser never opens raw IMAP or SMTP network sockets. Native secret APIs remain off-limits to email HTML rendering.
