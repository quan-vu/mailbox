# Real mail provider used for testing (a hosted business-mail provider)

> Host names, the domain and the test address in this file are placeholders (`*.provider.example`, `example.org`). The real values are kept by the owner outside the repository.

Owner-provided on 2026-11-14 for testing MailBox against a real IMAP/SMTP provider. This is a hosted business-mail service (private mail server, Dovecot), the main case in the PRD.

**No password is stored in this repository.** Credentials live only in the owner's git-ignored `.env.smtp` (SMTP) and are typed by the owner into the Desktop app (IMAP). Agents must not read or print secret values from that file. Running the project's own SMTP tests, which load the file inside the test process, was authorised by the owner on 2026-11-14.

## Test mailbox

| Item | Value |
|---|---|
| Domain | `example.org` |
| Test address | `test@example.org` (used for testing only) |
| Login name | the full email address |
| Authentication | mailbox password (IMAP `AUTH=PLAIN`/`LOGIN`; no OAuth) |

## Client settings

Encrypted (use these; MailBox only supports encrypted connections):

| Protocol | Host | Port | Security |
|---|---|---|---|
| IMAPS | `mail.provider.example` | 993 | Implicit TLS |
| SMTPS | `mail.provider.example` | 465 | Implicit TLS |
| SMTP submission | `mail.provider.example` | 587 | STARTTLS |
| POP3S | `mail.provider.example` | 995 | Implicit TLS (not used by MailBox) |

Unencrypted endpoints also exist at the provider (`mail.example.org`, ports 110/143/25, no SSL). **MailBox must never use them**: they are listed only so nobody "fixes" a connection problem by switching to them.

Use the provider host name (`mail.provider.example`), not `mail.example.org`, for TLS: the certificate is issued for the provider's name. A certificate error on `mail.example.org:993` is expected and must not be bypassed.

## DNS records of the domain (reference)

| Host | Type | Value | Priority |
|---|---|---|---|
| `@` | MX | `mail.provider.example` | 5 |
| `@` | MX | `mx.provider.example` | 10 |
| `mail` | CNAME | `mail.provider.example` | |
| `mx` | CNAME | `mx.provider.example` | |
| `@` | TXT | `v=spf1 include:spf.provider.example ~all` | |

DKIM values are issued by the provider's support team on request.

## Two test mailboxes, same provider

The owner uses two mailboxes at one provider. Do not mix them up:

| Mailbox | Mail server | Where its password is | Used for |
|---|---|---|---|
| `test@example.org` | `mail.provider.example` | typed by the owner into the Desktop app | manual check in the app |
| the account in `.env.smtp` (another domain) | `mail2.provider.example` | git-ignored `.env.smtp` | automated owner tests (SMTP login/send, IMAP sync) |

The settings table above (ports, security) applies to both; only the host differs.

## What has been verified against this provider

| Check | Result | Date |
|---|---|---|
| TLS certificate valid for `mail.provider.example:993` | Pass | 2026-11-14 |
| IMAP greeting and capabilities (no credentials sent) | `IMAP4rev1 SASL-IR LOGIN-REFERRALS ID ENABLE IDLE LITERAL+ AUTH=PLAIN AUTH=LOGIN` | 2026-11-14 |
| IMAP login for `test@example.org` | Not yet — to be done by the owner in the Desktop app | |
| SMTP 465 (implicit TLS) and 587 (STARTTLS): certificate + capabilities, no credentials | Pass — `PIPELINING, SIZE 52428800, ETRN, AUTH PLAIN LOGIN, ENHANCEDSTATUSCODES, 8BITMIME, DSN` | 2026-11-14 |
| SMTP login with the `.env.smtp` account on `mail2.provider.example` | Pass (port 465; From matches login) | 2026-11-14 |
| SMTP send from the `.env.smtp` account: one message to the owner's own address, one to the test mailbox itself | Both `Accepted`; the self-addressed one was then found by IMAP sync | 2026-11-14 |
| IMAP login + folder discovery + sync into SQLite with the `.env.smtp` account (read-only) | Pass — 5 folders (inbox, sent, drafts, trash, junk); 1 message synced with subject, sender and date; second sync incremental with 0 new | 2026-11-14 |

Reproduce the credential-free probes:

```sh
MAILBOX_TEST_IMAP_HOST=mail.provider.example \
  cargo test -p mailbox-imap --test network -- --ignored --nocapture
MAILBOX_TEST_SMTP_HOST=mail.provider.example \
  cargo test -p mailbox-smtp --test network probe -- --ignored --nocapture
```

Owner-only tests (read `.env.smtp` themselves; `owner_send` sends one real email, `self` meaning the test mailbox itself):

```sh
cargo test -p mailbox-smtp --test network owner_login -- --ignored --nocapture
MAILBOX_SMTP_SEND_TO=<your address or self> \
  cargo test -p mailbox-smtp --test network owner_send -- --ignored --nocapture
MAILBOX_TEST_IMAP_HOST=mail2.provider.example \
  cargo test -p mailbox-store --test real_sync -- --ignored --nocapture
```

Note: this provider takes about 9 seconds to complete the TLS handshake, so connection checks feel slow; the step timeout is 30 seconds.

## `.env.smtp` (git-ignored, owner's machine only)

Keys expected by SMTP tests the owner runs locally: `SMTP_HOST_1`, `SMTP_PORT_1`, `SMTP_TLS_1`, `SMTP_USER_1`, `SMTP_PASS_1`, `SMTP_FROM_1`. The file matches `.env.*` in `.gitignore`; never commit it and never print `SMTP_PASS_1`.
