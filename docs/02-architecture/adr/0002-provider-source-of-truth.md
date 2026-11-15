# ADR-0002 — Remote mail service is authoritative

Date: 2026-11-14 • Status: Accepted

**Decision**: MailBox is a client, not MX/mail hosting. IMAP server is authoritative for folders/messages/flags. SMTP submission server accepts mail sending. Caches are disposable/rebuildable and never used as sole source for remote data.

**Consequences**: Requires robust IMAP reconciliation, partial failures UI and careful local draft ownership. Unified Inbox is a virtual view. Cross-account move is not one server mutation.
