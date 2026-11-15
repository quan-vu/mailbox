//! Composer state. Pure logic shared by Demo and Real mode: recipients, reply/forward prefill,
//! threading headers, what blocks a send. Demo drafts live only in memory and are never sent;
//! in Real mode (Desktop) the draft is saved locally and sent through `state::outbox`.
use crate::i18n::{bi, tr};
use mailbox_compose::address::{is_valid_address, parse_mailbox, split_list};
use mailbox_compose::reply::{forward_subject, quote, reply_subject};
use mailbox_contracts::MessageSummary;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecipientField {
    To,
    Cc,
    Bcc,
}

impl RecipientField {
    pub fn key(self) -> &'static str {
        match self {
            RecipientField::To => "to",
            RecipientField::Cc => "cc",
            RecipientField::Bcc => "bcc",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipient {
    pub address: String,
    pub display: String,
    pub valid: bool,
}

impl Recipient {
    // Used by the Desktop draft store only.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    /// The recipient as one list entry (`addr` or `"Name" <addr>`), the form drafts are stored in.
    pub fn entry(&self) -> String {
        if self.display.is_empty() || self.display == self.address {
            self.address.clone()
        } else {
            format!(
                "\"{}\" <{}>",
                self.display.replace('\\', "\\\\").replace('"', "\\\""),
                self.address
            )
        }
    }
}

/// Structural check only (local-part@domain.tld). Deliverability is the SMTP server's job.
/// Same rule as the message builder (`mailbox-compose`), so a green chip can always be sent.
pub fn is_valid_email(input: &str) -> bool {
    is_valid_address(input.trim())
}

/// Accepts `name@host` or `Name <name@host>`; invalid entries are kept and flagged.
pub fn parse_recipient(input: &str) -> Option<Recipient> {
    let raw = input.trim().trim_end_matches([',', ';']).trim();
    if raw.is_empty() {
        return None;
    }
    Some(match parse_mailbox(raw) {
        Ok(m) => Recipient {
            display: m.display().to_string(),
            address: m.address,
            valid: true,
        },
        Err(_) => {
            // Keep what was typed so the user can see and fix it; never keep control characters.
            let shown: String = raw.chars().filter(|c| !c.is_control()).collect();
            Recipient {
                address: shown.clone(),
                display: shown,
                valid: false,
            }
        }
    })
}

/// A file attached to the draft. In Real mode it is a reference to a local file, read when the
/// message is sent, or (forward) a part of a cached message, fetched again when sending. With
/// neither (demo, or a forward whose original body is not downloaded) it cannot be sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftAttachment {
    pub name: String,
    pub size: u64,
    pub path: Option<String>,
    pub forwarded: Option<mailbox_core::outbox::ForwardedPart>,
}

impl DraftAttachment {
    /// Can be put into a real message (local file or forwarded part).
    pub fn sendable(&self) -> bool {
        self.path.is_some() || self.forwarded.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Draft {
    pub from_account: String,
    pub to: Vec<Recipient>,
    pub cc: Vec<Recipient>,
    pub bcc: Vec<Recipient>,
    pub subject: String,
    pub body: String,
    pub attachments: Vec<DraftAttachment>,
    /// When replying, the account that received the original message.
    pub reply_account: Option<String>,
    /// Id in the local draft store. Empty until the first save (Real mode only).
    pub id: String,
    /// Version returned by the last save, for conditional updates.
    pub version: u32,
    /// `Message-ID` given to this draft when it was first saved; it never changes afterwards.
    pub message_id: String,
    /// Reply/forward: id of the message being answered and how.
    pub reply_source: Option<(String, ReplyKind)>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyKind {
    Reply,
    ReplyAll,
    Forward,
}

/// Body text that may be quoted. A real message whose body is not downloaded yet has an empty
/// text and is not quoted.
pub fn quotable_body(original: &MessageSummary) -> Option<&str> {
    let body = original.body_plain.trim();
    (!body.is_empty()).then_some(body)
}

/// `In-Reply-To` and `References` for a reply or forward, from the original's own headers.
/// A reply answers the original; a forward only refers to it.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn threading(
    kind: ReplyKind,
    original_message_id: Option<&str>,
    original_references: &[String],
) -> (Option<String>, Vec<String>) {
    let Some(id) = original_message_id
        .map(str::trim)
        .filter(|id| !id.is_empty())
    else {
        return (None, Vec::new());
    };
    let mut references: Vec<String> = original_references
        .iter()
        .filter(|r| r.as_str() != id)
        .cloned()
        .collect();
    references.push(id.to_string());
    let in_reply_to = (kind != ReplyKind::Forward).then(|| id.to_string());
    (in_reply_to, references)
}

/// What pressing Send does. Demo never submits anything, whatever the draft looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendAction {
    ShowBlockers,
    /// Explain that nothing is sent.
    DemoNotice,
    /// Hand the draft to the outbox (Real mode on Desktop only).
    Submit,
}

pub fn send_action(real: bool, can_send_here: bool, blockers: usize) -> SendAction {
    if blockers > 0 {
        SendAction::ShowBlockers
    } else if real && can_send_here {
        SendAction::Submit
    } else {
        SendAction::DemoNotice
    }
}

impl Draft {
    pub fn new(default_account: &str) -> Self {
        Self {
            from_account: default_account.to_string(),
            ..Self::default()
        }
    }

    /// Reply defaults the sender identity to the account that received the original.
    pub fn reply_to(original: &MessageSummary, kind: ReplyKind) -> Self {
        let mut d = Draft::new(&original.account_id);
        d.reply_account = Some(original.account_id.clone());
        d.reply_source = Some((original.id.clone(), kind));
        match kind {
            ReplyKind::Reply | ReplyKind::ReplyAll => {
                d.subject = reply_subject(&original.subject);
                d.to.push(Recipient {
                    address: original.from_address.clone(),
                    display: original.from_name.clone(),
                    valid: is_valid_email(&original.from_address),
                });
                if kind == ReplyKind::ReplyAll {
                    // BCC of the original is never known to us and never copied.
                    d.add_recipient(RecipientField::Cc, &original.to_line);
                    let sender = original.from_address.clone();
                    d.cc.retain(|r| !r.address.eq_ignore_ascii_case(&sender));
                }
            }
            ReplyKind::Forward => {
                d.subject = forward_subject(&original.subject);
                // Names only: the files are not on this device, so they cannot be re-sent yet.
                d.attachments = original
                    .attachments
                    .iter()
                    .map(|a| DraftAttachment {
                        name: a.filename.clone(),
                        size: a.size_bytes,
                        path: None,
                        forwarded: None,
                    })
                    .collect();
            }
        }
        let attribution = bi(
            format!(
                "--- {} <{}> đã viết ({}):",
                original.from_name, original.from_address, original.received_at_full
            ),
            format!(
                "--- {} <{}> wrote ({}):",
                original.from_name, original.from_address, original.received_at_full
            ),
        );
        d.body = match quotable_body(original) {
            Some(body) => quote(&attribution, body),
            None => String::new(),
        };
        d
    }

    pub fn field_mut(&mut self, field: RecipientField) -> &mut Vec<Recipient> {
        match field {
            RecipientField::To => &mut self.to,
            RecipientField::Cc => &mut self.cc,
            RecipientField::Bcc => &mut self.bcc,
        }
    }

    pub fn add_recipient(&mut self, field: RecipientField, input: &str) -> bool {
        let mut added = false;
        for part in split_list(input) {
            if let Some(r) = parse_recipient(&part) {
                let list = self.field_mut(field);
                if !list
                    .iter()
                    .any(|x| x.address.eq_ignore_ascii_case(&r.address))
                {
                    list.push(r);
                    added = true;
                }
            }
        }
        added
    }

    pub fn remove_recipient(&mut self, field: RecipientField, index: usize) {
        let list = self.field_mut(field);
        if index < list.len() {
            list.remove(index);
        }
    }

    /// Reply-all must not copy the user's own addresses back to themselves.
    pub fn drop_own_addresses(&mut self, own: &[String]) {
        let mine = |r: &Recipient| own.iter().any(|a| a.eq_ignore_ascii_case(&r.address));
        self.cc.retain(|r| !mine(r));
        if self.to.len() > 1 {
            self.to.retain(|r| !mine(r));
        }
    }

    /// True when the user picked a different identity than the account that received the original.
    pub fn identity_mismatch(&self) -> bool {
        self.reply_account
            .as_deref()
            .is_some_and(|a| a != self.from_account)
    }

    /// Nothing the user would lose: no recipient, subject, attachment, and a body that is blank
    /// or exactly one of the user's stored signatures (read from the preferences when the app is
    /// running).
    pub fn is_empty(&self) -> bool {
        let known = known_signatures();
        let known: Vec<&str> = known.iter().map(String::as_str).collect();
        self.is_empty_with(&known)
    }

    /// [`Draft::is_empty`] with the signature texts given explicitly.
    pub fn is_empty_with(&self, known_signatures: &[&str]) -> bool {
        self.to.is_empty()
            && self.cc.is_empty()
            && self.bcc.is_empty()
            && self.subject.trim().is_empty()
            && body_is_blank_with(&self.body, known_signatures)
            && self.attachments.is_empty()
    }

    pub fn total_attachment_bytes(&self) -> u64 {
        self.attachments.iter().map(|a| a.size).sum()
    }

    /// Problems that block a send, in display order.
    pub fn send_blockers(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        let all = self.to.iter().chain(&self.cc).chain(&self.bcc);
        if self.to.is_empty() && self.cc.is_empty() && self.bcc.is_empty() {
            out.push(tr("Thêm ít nhất một người nhận."));
        }
        if all.clone().any(|r| !r.valid) {
            out.push(tr("Có địa chỉ email không hợp lệ."));
        }
        if self.subject.trim().is_empty() {
            out.push(tr("Chủ đề đang trống."));
        }
        if self.subject.chars().any(|c| c.is_control() && c != '\t') {
            out.push(tr("Chủ đề có ký tự xuống dòng hoặc ký tự điều khiển."));
        }
        if self.total_attachment_bytes() > ATTACHMENT_LIMIT_BYTES {
            out.push(tr("Tệp đính kèm vượt giới hạn 25 MB."));
        }
        out
    }

    /// Blockers for a real send: everything above, plus attachments that are not local files.
    pub fn real_send_blockers(&self) -> Vec<&'static str> {
        let mut out = self.send_blockers();
        if self.attachments.iter().any(|a| !a.sendable()) {
            out.push(tr(
                "Có tệp đính kèm chưa nằm trên máy này. Gỡ tệp đó hoặc đính kèm lại từ máy.",
            ));
        }
        if self.from_account.is_empty() {
            out.push(tr("Chọn tài khoản gửi."));
        }
        out
    }
}

/// A body with no text of the user's: empty, or exactly one of the stored signatures.
pub fn body_is_blank(body: &str) -> bool {
    let known = known_signatures();
    let known: Vec<&str> = known.iter().map(String::as_str).collect();
    body_is_blank_with(body, &known)
}

/// [`body_is_blank`] with the signature texts given explicitly.
pub fn body_is_blank_with(body: &str, known_signatures: &[&str]) -> bool {
    mailbox_core::signature::is_only_signature_of(body, known_signatures)
}

/// Signature texts from the stored preferences. Empty outside a running Dioxus app (unit tests),
/// which makes the emptiness checks keep any signature-shaped body.
fn known_signatures() -> Vec<String> {
    if dioxus::core::Runtime::try_current().is_none() {
        return Vec::new();
    }
    use dioxus::prelude::ReadableExt;
    super::prefs::PREFS
        .peek()
        .signatures
        .values()
        .map(|s| s.text.clone())
        .collect()
}

pub const ATTACHMENT_LIMIT_BYTES: u64 = 25 * 1024 * 1024;

pub fn format_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0)).replace('.', ",")
    } else {
        format!("{} KB", bytes.div_ceil(1024))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Template {
    pub key: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub icon: &'static str,
    pub subject: &'static str,
    pub body: &'static str,
}

pub const TEMPLATES: [Template; 5] = [
    Template {
        key: "launch",
        title: "Kế hoạch ra mắt sản phẩm",
        description: "Thông báo kế hoạch, timeline và phân công",
        icon: "mail",
        subject: "Kế hoạch ra mắt sản phẩm",
        body: "Chào team,\n\nMình gửi kế hoạch ra mắt gồm timeline, phạm vi và phân công.\n\n• Thời gian ra mắt:\n• Phạm vi:\n• Việc tiếp theo:\n\nCảm ơn cả team!",
    },
    Template {
        key: "announce",
        title: "Thông báo nội bộ",
        description: "Cập nhật thông tin quan trọng cho team",
        icon: "megaphone",
        subject: "Thông báo nội bộ",
        body: "Chào mọi người,\n\nMình xin thông báo:\n\nTrân trọng.",
    },
    Template {
        key: "meeting",
        title: "Mời họp dự án",
        description: "Lời mời họp và nội dung thảo luận",
        icon: "calendar",
        subject: "Mời họp dự án",
        body: "Chào mọi người,\n\nMời mọi người tham gia buổi họp:\n• Thời gian:\n• Nội dung:\n\nCảm ơn!",
    },
    Template {
        key: "report",
        title: "Báo cáo tiến độ",
        description: "Tổng hợp kết quả và việc tiếp theo",
        icon: "chart",
        subject: "Báo cáo tiến độ",
        body: "Chào team,\n\nKết quả tuần này:\n\nViệc tiếp theo:\n\nTrân trọng.",
    },
    Template {
        key: "thanks",
        title: "Cảm ơn & phản hồi",
        description: "Gửi lời cảm ơn và ghi nhận đóng góp",
        icon: "heart",
        subject: "Cảm ơn",
        body: "Chào bạn,\n\nCảm ơn bạn đã hỗ trợ.\n\nTrân trọng.",
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use mailbox_contracts::fixtures;

    /// MB-304: a draft is empty only when its body is exactly a stored signature; text typed
    /// below the signature, or a body that merely starts with `-- `, keeps the draft.
    #[test]
    fn draft_with_only_the_signature_is_empty_but_real_text_is_kept() {
        let known = ["Linh"];
        let mut d = Draft::new("acc-1");
        assert!(d.is_empty_with(&known));
        d.body = mailbox_core::signature::block("Linh");
        assert!(d.is_empty_with(&known), "only the automatic signature");
        d.body.push_str("\n\nNội dung gõ dưới chữ ký");
        assert!(
            !d.is_empty_with(&known),
            "text below the signature must be saved"
        );
        d.body = "-- \nghi chú riêng".into();
        assert!(
            !d.is_empty_with(&known),
            "a note starting with `-- ` is real text"
        );
        assert!(body_is_blank_with("\n\n-- \nLinh", &known));
        assert!(!body_is_blank_with("\n\n-- \nLinh", &[]));
    }

    /// Outside the running app no signature is known, so a signature-shaped body is kept
    /// rather than treated as empty (no data loss when in doubt).
    #[test]
    fn without_known_signatures_a_signature_shaped_body_is_not_empty() {
        let mut d = Draft::new("acc-1");
        d.body = mailbox_core::signature::block("Linh");
        assert!(!d.is_empty());
        assert!(!body_is_blank(&d.body));
        d.body = "  \n".into();
        assert!(d.is_empty());
    }

    #[test]
    fn email_validation_is_structural() {
        assert!(is_valid_email("linh.tran@company.vn"));
        assert!(!is_valid_email("ketoan@company"));
        assert!(!is_valid_email("no-at-sign.vn"));
        assert!(!is_valid_email("a b@company.vn"));
        assert!(!is_valid_email("@company.vn"));
        assert!(!is_valid_email("a@.vn"));
    }

    #[test]
    fn parse_named_and_invalid_recipients() {
        let r = parse_recipient("Trần Linh <linh.tran@company.vn>").unwrap();
        assert_eq!(
            (r.display.as_str(), r.address.as_str(), r.valid),
            ("Trần Linh", "linh.tran@company.vn", true)
        );
        let bad = parse_recipient("ketoan@company").unwrap();
        assert!(!bad.valid);
        assert!(parse_recipient("   ").is_none());
    }

    #[test]
    fn add_recipient_splits_and_dedupes() {
        let mut d = Draft::new("work");
        assert!(d.add_recipient(RecipientField::To, "a@x.vn, b@x.vn; A@X.VN"));
        assert_eq!(d.to.len(), 2);
        d.remove_recipient(RecipientField::To, 0);
        assert_eq!(d.to[0].address, "b@x.vn");
    }

    #[test]
    fn chips_accept_exactly_what_the_message_builder_accepts() {
        let mut d = Draft::new("work");
        d.add_recipient(
            RecipientField::To,
            "\"Trần, Linh\" <linh@company.vn>; x@y.vn\r\nBcc: victim@evil.io, bad@host",
        );
        let shown: Vec<(&str, bool)> = d.to.iter().map(|r| (r.address.as_str(), r.valid)).collect();
        assert_eq!(
            shown,
            vec![
                ("linh@company.vn", true),
                ("x@y.vn", true),
                ("Bcc: victim@evil.io", false),
                ("bad@host", false),
            ],
            "a quoted comma stays in the name; a pasted header line is an invalid chip, not a recipient"
        );
        assert_eq!(d.to[0].display, "Trần, Linh");
        for r in d.to.iter().filter(|r| r.valid) {
            let parsed = mailbox_compose::address::parse_mailbox(&r.entry()).unwrap();
            assert_eq!(
                (parsed.address, parsed.name),
                (
                    r.address.clone(),
                    if r.display == r.address {
                        String::new()
                    } else {
                        r.display.clone()
                    }
                )
            );
        }
        assert_eq!(
            d.send_blockers(),
            vec!["Có địa chỉ email không hợp lệ.", "Chủ đề đang trống."]
        );
    }

    #[test]
    fn reply_uses_receiving_account_and_never_leaks_bcc() {
        let data = fixtures::bootstrap();
        let original = data.messages.iter().find(|m| m.id == "m07").unwrap();
        let d = Draft::reply_to(original, ReplyKind::ReplyAll);
        assert_eq!(d.from_account, "support");
        assert_eq!(d.subject, "Re: Phản hồi về đề xuất hợp tác");
        assert_eq!(d.to[0].address, "contact@greentech.example");
        assert!(d.bcc.is_empty());
        assert_eq!(
            d.reply_source,
            Some(("m07".to_string(), ReplyKind::ReplyAll))
        );
        let mut changed = d.clone();
        changed.from_account = "work".into();
        assert!(changed.identity_mismatch());
        assert!(!d.identity_mismatch());
    }

    #[test]
    fn reply_quotes_the_original_and_reply_all_drops_own_address() {
        let data = fixtures::bootstrap();
        let mut original = data
            .messages
            .iter()
            .find(|m| m.id == "m07")
            .unwrap()
            .clone();
        original.subject = "RE: Re: Báo giá".into();
        original.body_plain = "Dòng một\n\nDòng hai".into();
        original.to_line = "Me <me@company.vn>, An <an@partner.vn>".into();
        let mut d = Draft::reply_to(&original, ReplyKind::ReplyAll);
        assert_eq!(d.subject, "Re: Báo giá", "prefixes do not stack");
        assert!(
            d.body.starts_with("\n\n--- "),
            "room to type above the quote"
        );
        assert!(d.body.ends_with("> Dòng một\n>\n> Dòng hai\n"));
        d.drop_own_addresses(&["ME@company.vn".to_string()]);
        assert_eq!(
            d.cc.iter().map(|r| r.address.as_str()).collect::<Vec<_>>(),
            vec!["an@partner.vn"]
        );
        assert_eq!(d.to.len(), 1, "the person being answered always stays");
    }

    #[test]
    fn a_body_that_was_never_downloaded_is_not_quoted() {
        let data = fixtures::bootstrap();
        let mut original = data.messages[0].clone();
        original.body_plain = String::new();
        for kind in [ReplyKind::Reply, ReplyKind::Forward] {
            let d = Draft::reply_to(&original, kind);
            assert_eq!(d.body, "", "{kind:?}");
        }
    }

    #[test]
    fn threading_headers_follow_the_original() {
        let refs = vec!["<root@x>".to_string(), "<mid@x>".to_string()];
        assert_eq!(
            threading(ReplyKind::Reply, Some("<orig@x>"), &refs),
            (
                Some("<orig@x>".into()),
                vec!["<root@x>".into(), "<mid@x>".into(), "<orig@x>".into()]
            )
        );
        assert_eq!(
            threading(ReplyKind::Forward, Some("<orig@x>"), &[]),
            (None, vec!["<orig@x>".into()]),
            "a forward refers to the original but does not answer it"
        );
        // No duplicate when the original already lists itself; nothing invented without an id.
        assert_eq!(
            threading(ReplyKind::ReplyAll, Some("<mid@x>"), &refs).1,
            vec!["<root@x>", "<mid@x>"]
        );
        assert_eq!(threading(ReplyKind::Reply, None, &refs), (None, vec![]));
        assert_eq!(
            threading(ReplyKind::Reply, Some("  "), &refs),
            (None, vec![])
        );
    }

    #[test]
    fn forward_carries_attachments_and_prefix() {
        let data = fixtures::bootstrap();
        let original = data.messages.iter().find(|m| m.id == "m01").unwrap();
        let d = Draft::reply_to(original, ReplyKind::Forward);
        assert_eq!(d.subject, "Fwd: Kế hoạch ra mắt phiên bản 2.0");
        assert_eq!(d.attachments.len(), 2);
        assert!(d.to.is_empty());
        assert!(d.in_reply_to.is_none());
        // Forwarded files are not on this device: a real send is blocked until they are removed.
        assert!(d.attachments.iter().all(|a| a.path.is_none()));
        assert!(
            d.real_send_blockers()
                .iter()
                .any(|b| b.contains("chưa nằm trên máy"))
        );
    }

    #[test]
    fn send_blockers_explain_what_is_missing() {
        let mut d = Draft::new("work");
        assert_eq!(d.send_blockers().len(), 2);
        d.add_recipient(RecipientField::To, "ketoan@company");
        d.subject = "Hi".into();
        assert_eq!(d.send_blockers(), vec!["Có địa chỉ email không hợp lệ."]);
        d.remove_recipient(RecipientField::To, 0);
        d.add_recipient(RecipientField::To, "ketoan@company.vn");
        assert!(d.send_blockers().is_empty());
        assert!(d.real_send_blockers().is_empty());
        d.subject = "Hi\r\nBcc: x@y.vn".into();
        assert_eq!(
            d.send_blockers().len(),
            1,
            "a line break in the subject blocks the send"
        );
        d.subject = "Hi".into();
        d.attachments.push(DraftAttachment {
            name: "big.bin".into(),
            size: ATTACHMENT_LIMIT_BYTES + 1,
            forwarded: None,
            path: Some("/tmp/big.bin".into()),
        });
        assert_eq!(
            d.real_send_blockers(),
            vec!["Tệp đính kèm vượt giới hạn 25 MB."]
        );
    }

    #[test]
    fn demo_never_submits_and_real_submits_only_a_clean_draft() {
        for blockers in [0, 2] {
            for can_send in [false, true] {
                assert_ne!(
                    send_action(false, can_send, blockers),
                    SendAction::Submit,
                    "demo"
                );
            }
        }
        assert_eq!(send_action(false, true, 0), SendAction::DemoNotice);
        assert_eq!(send_action(true, true, 1), SendAction::ShowBlockers);
        assert_eq!(send_action(true, true, 0), SendAction::Submit);
        // Real mode on a build without a sender (Web) explains instead of pretending.
        assert_eq!(send_action(true, false, 0), SendAction::DemoNotice);
    }

    #[test]
    fn sizes_are_human_readable() {
        assert_eq!(format_size(3_355_443), "3,2 MB");
        assert_eq!(format_size(220_000), "215 KB");
    }
}
