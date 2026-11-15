# MailBox documentation index

Version: **v0.1 starter / 2026-11-14**. Docs use Vietnamese where helpful, identifiers in English.

| Path | Description | Owner |
|---|---|---|
| [`00-product/VISION.md`](00-product/VISION.md) | Product vision, personas, differentiation | Product |
| [`00-product/PRD.md`](00-product/PRD.md) | Goals, scope, MVP, release criteria | Product |
| [`01-features/FEATURES.md`](01-features/FEATURES.md) | Functional specification, IDs, acceptance | Product/Eng |
| [`01-features/USER_JOURNEYS.md`](01-features/USER_JOURNEYS.md) | Critical user flows and edge cases | UX |
| [`02-architecture/SYSTEM_DESIGN.md`](02-architecture/SYSTEM_DESIGN.md) | System/context/container diagrams, boundaries | Architect |
| [`02-architecture/MAIL_SYNC.md`](02-architecture/MAIL_SYNC.md) | IMAP protocol, concurrency, recovery | Backend |
| [`02-architecture/DATA_MODEL.md`](02-architecture/DATA_MODEL.md) | Tables, keys, lifetimes, indexes | Backend |
| [`02-architecture/API.md`](02-architecture/API.md) | API contract/response errors/versioning | Backend |
| [`02-architecture/adr/`](02-architecture/adr/) | Accepted architectural decisions | Architect |
| [`03-ui/DESIGN_SYSTEM.md`](03-ui/DESIGN_SYSTEM.md) | Tokens, components, typography, patterns | UI |
| [`03-ui/SCREENS.md`](03-ui/SCREENS.md) | Five reference screen specs and states | UX/UI |
| [`03-ui/RESPONSIVE_ACCESSIBILITY.md`](03-ui/RESPONSIVE_ACCESSIBILITY.md) | Responsive, keyboard, contrast, a11y | UI |
| [`03-ui/references/`](03-ui/references/) | Five high-fidelity PNG designs | UI |
| [`04-security/THREAT_MODEL.md`](04-security/THREAT_MODEL.md) | Attack surfaces and controls | Security |
| [`04-security/PRIVACY.md`](04-security/PRIVACY.md) | Retention, logging, encryption, account removal | Security |
| [`05-engineering/QUALITY_GATES.md`](05-engineering/QUALITY_GATES.md) | Testing, CI, performance, release | Eng |
| [`05-engineering/DEPENDENCIES.md`](05-engineering/DEPENDENCIES.md) | Dependency/compatibility decisions | Eng |
| [`05-engineering/TEST_MAIL_PROVIDER.md`](05-engineering/TEST_MAIL_PROVIDER.md) | Real provider (hosted business mail) settings for testing; no secrets | Eng |
| [`05-engineering/DEPLOYMENT.md`](05-engineering/DEPLOYMENT.md) | Web/Desktop release and operations | DevOps |
| [`06-delivery/ROADMAP.md`](06-delivery/ROADMAP.md) | Ordered phases and release gates | Lead |
| [`06-delivery/TRACKING.md`](06-delivery/TRACKING.md) | Numbered tasks with state and dependencies | All |
| [`06-delivery/AGENT_HANDOFF.md`](06-delivery/AGENT_HANDOFF.md) | Agent prompt, checkpoints, questions | Agent |

**Status legend:** `NOT STARTED`, `IN PROGRESS`, `BLOCKED`, `DONE`. Only use `DONE` with test evidence. UI reference images are not running screenshots.

Machine-readable tasks: [`06-delivery/TASKS.json`](06-delivery/TASKS.json) and issue import [`06-delivery/ISSUES_IMPORT.csv`](06-delivery/ISSUES_IMPORT.csv).
