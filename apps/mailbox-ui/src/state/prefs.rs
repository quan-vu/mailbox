//! Real-mode settings and templates (MB-210). `PREFS` is the working copy the Settings screen
//! edits; `SAVED` is what is stored. Both are global signals so `AppState` needs no new field.
//! On the Web build nothing is stored: the screens say so instead of pretending.
use super::{AppState, Theme};
use crate::i18n::tr;
use dioxus::prelude::*;
use mailbox_core::prefs::{
    Preferences, Template, TemplateContext, ThemeChoice, fill_subject, fill_template,
};

pub static PREFS: GlobalSignal<Preferences> = Signal::global(Preferences::default);
pub static SAVED: GlobalSignal<Preferences> = Signal::global(Preferences::default);
pub static USER_TEMPLATES: GlobalSignal<Vec<Template>> = Signal::global(Vec::new);
/// Set when stored settings could not be read; the defaults are shown and the user is told.
pub static PREFS_ERROR: GlobalSignal<Option<&'static str>> = Signal::global(|| None);

/// True when this build stores settings (Desktop).
pub const CAN_STORE: bool = cfg!(not(target_arch = "wasm32"));

pub fn theme_of(choice: ThemeChoice) -> Theme {
    match choice {
        ThemeChoice::Light => Theme::Light,
        ThemeChoice::Dark => Theme::Dark,
        ThemeChoice::System => Theme::System,
    }
}

/// Switch the interface language now and remember it (Desktop). Works in demo and real mode: the
/// stored record is read, the language changed and written back, so no unsaved edit of the
/// working copy is saved by accident.
pub fn choose_language(lang: crate::i18n::Lang) {
    crate::i18n::set_lang(lang);
    PREFS.write().lang = lang;
    SAVED.write().lang = lang;
    #[cfg(not(target_arch = "wasm32"))]
    spawn(async move {
        // A failed write only means the choice is not remembered; the running UI already switched.
        let _ = crate::adapters::prefs::save_language(lang).await;
    });
}

pub fn is_dirty() -> bool {
    *PREFS.read() != *SAVED.read()
}

/// New template id: unique enough for one local user, safe as a table key.
pub fn new_template_id(now_nanos: u128) -> String {
    format!("tpl-{now_nanos:x}")
}

impl AppState {
    /// Make a settings record take effect in the running app.
    pub fn apply_prefs(mut self, prefs: &Preferences) {
        self.theme.set(theme_of(prefs.theme));
        let ids: Vec<String> = self
            .mail
            .peek()
            .accounts
            .iter()
            .map(|a| a.id.clone())
            .collect();
        if let Some(id) = prefs.default_identity.as_ref().filter(|i| ids.contains(i)) {
            self.default_from.set(id.clone());
        }
        // Auto-sync (MB-140 setting): starting or stopping the live workers follows at once.
        #[cfg(not(target_arch = "wasm32"))]
        if crate::adapters::worker::set_auto_sync(prefs.auto_sync)
            && *self.mode.peek() == super::AppMode::Real
            && let Ok(rt) = tokio::runtime::Handle::try_current()
        {
            rt.spawn(crate::adapters::worker::ensure_running());
        }
    }

    /// Read stored settings and templates (Real mode, Desktop). A read failure keeps defaults.
    #[allow(unused_mut)] // only mutated on native targets
    pub async fn load_prefs(mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        match crate::adapters::prefs::load().await {
            Ok((prefs, templates)) => {
                *PREFS_ERROR.write() = None;
                crate::i18n::set_lang(prefs.lang);
                self.apply_prefs(&prefs);
                *PREFS.write() = prefs.clone();
                *SAVED.write() = prefs;
                *USER_TEMPLATES.write() = templates;
            }
            Err(_) => {
                *PREFS_ERROR.write() = Some(tr(
                    "Không đọc được cài đặt đã lưu. Đang dùng giá trị mặc định; cài đặt cũ không bị xóa.",
                ));
                let d = Preferences::default();
                self.apply_prefs(&d);
                *PREFS.write() = d.clone();
                *SAVED.write() = d;
            }
        }
        #[cfg(target_arch = "wasm32")]
        let _ = &mut self;
    }

    /// Edit the working copy; the change shows at once and is stored by "Lưu thay đổi".
    pub fn edit_prefs(self, change: impl FnOnce(&mut Preferences)) {
        let mut next = PREFS.peek().clone();
        change(&mut next);
        self.apply_prefs(&next);
        *PREFS.write() = next;
    }

    pub async fn save_prefs(self) {
        let prefs = PREFS.peek().clone();
        #[cfg(not(target_arch = "wasm32"))]
        match crate::adapters::prefs::save(&prefs).await {
            Ok(()) => {
                *SAVED.write() = prefs;
                self.notify(tr("Đã lưu cài đặt trên máy này."), false);
            }
            Err(_) => self.notify(tr("Không lưu được cài đặt. Thử lại."), false),
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (prefs, self);
    }

    pub async fn restore_prefs(self) {
        let next = PREFS.peek().restore_defaults();
        #[cfg(not(target_arch = "wasm32"))]
        match crate::adapters::prefs::save(&next).await {
            Ok(()) => {
                self.apply_prefs(&next);
                *PREFS.write() = next.clone();
                *SAVED.write() = next;
                self.notify(
                    tr("Đã khôi phục mặc định. Chữ ký và tài khoản gửi mặc định được giữ nguyên."),
                    false,
                );
            }
            Err(_) => self.notify(tr("Không khôi phục được cài đặt. Thử lại."), false),
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (next, self);
    }

    /// Insert or update a template; the list shown is re-read from the store.
    pub async fn save_template(self, t: Template) -> bool {
        if let Some(problem) = t.problem() {
            self.notify(tr(problem), false);
            return false;
        }
        #[cfg(not(target_arch = "wasm32"))]
        match crate::adapters::prefs::save_template(&t).await {
            Ok(list) => {
                *USER_TEMPLATES.write() = list;
                self.notify(tr("Đã lưu mẫu thư."), false);
                true
            }
            Err(mailbox_core::MailboxError::Storage("template_limit")) => {
                self.notify(tr("Đã đạt giới hạn 100 mẫu. Xóa bớt mẫu cũ."), false);
                false
            }
            Err(_) => {
                self.notify(tr("Không lưu được mẫu thư."), false);
                false
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = t;
            false
        }
    }

    pub async fn delete_template(self, id: String) {
        #[cfg(not(target_arch = "wasm32"))]
        match crate::adapters::prefs::delete_template(&id).await {
            Ok(list) => {
                *USER_TEMPLATES.write() = list;
                self.notify(tr("Đã xóa mẫu thư."), false);
            }
            Err(_) => self.notify(tr("Không xóa được mẫu thư."), false),
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (id, self);
    }
}

/// The values a template can use, from the draft's first recipient.
pub fn template_context(draft: &super::compose::Draft) -> Option<TemplateContext> {
    draft
        .to
        .iter()
        .chain(&draft.cc)
        .find(|r| r.valid)
        .map(|r| TemplateContext::from_recipient(&r.display, &r.address))
}

/// What applying `t` to `draft` produces: filled subject and body, signature kept.
pub fn apply_template_to(
    draft: &super::compose::Draft,
    t: &Template,
    prefs: &Preferences,
) -> (String, String) {
    let ctx = template_context(draft);
    let subject = fill_subject(&t.subject, ctx.as_ref());
    let text = fill_template(&t.body, ctx.as_ref());
    let body = sync_signature_in(&text, draft, prefs);
    (subject, body)
}

/// The body with the signature of the draft's From identity and no other.
pub fn sync_signature_in(body: &str, draft: &super::compose::Draft, prefs: &Preferences) -> String {
    let is_reply = draft.reply_source.is_some();
    let desired = prefs
        .signature_for(&draft.from_account)
        .filter(|s| !is_reply || s.use_for_replies)
        .map(|s| s.text.as_str());
    let known: Vec<&str> = prefs.signatures.values().map(|s| s.text.as_str()).collect();
    mailbox_core::signature::sync(body, desired, &known)
}

impl AppState {
    pub fn apply_user_template(mut self, id: &str) {
        let Some(t) = USER_TEMPLATES.peek().iter().find(|t| t.id == id).cloned() else {
            return;
        };
        let prefs = PREFS.peek().clone();
        let mut draft = self.draft.write();
        let (subject, body) = apply_template_to(&draft, &t, &prefs);
        draft.subject = subject;
        draft.body = body;
    }
}

/// Real mode: keep the draft's signature in step with the From identity and the stored
/// signatures. Runs when the draft, its identity or the signatures change; never touches
/// the text the user typed and never adds a second copy.
pub fn use_signature_sync(app: AppState) {
    let key = use_memo(move || {
        let d = app.draft.read();
        // Not the draft id: it changes at the first save and must not bring back a deleted signature.
        (
            d.from_account.clone(),
            d.reply_source.as_ref().map(|(id, _)| id.clone()),
        )
    });
    use_effect(move || {
        let _ = key();
        let prefs = PREFS.read().clone();
        if !app.is_real() {
            return;
        }
        let mut draft = app.draft;
        let current = draft.peek().clone();
        if current.from_account.is_empty() {
            return;
        }
        let body = sync_signature_in(&current.body, &current, &prefs);
        if body != current.body {
            draft.write().body = body;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::super::compose::{Draft, RecipientField, ReplyKind};
    use super::*;
    use mailbox_core::prefs::Signature;

    fn prefs() -> Preferences {
        let mut p = Preferences::default();
        p.signatures.insert(
            "work".into(),
            Signature {
                text: "Work Linh".into(),
                use_for_replies: true,
            },
        );
        p.signatures.insert(
            "home".into(),
            Signature {
                text: "Home Linh".into(),
                use_for_replies: false,
            },
        );
        p
    }

    fn message() -> mailbox_contracts::MessageSummary {
        mailbox_contracts::MessageSummary {
            id: "m1".into(),
            account_id: "work".into(),
            folder_id: "inbox".into(),
            from_name: "An".into(),
            from_address: "an@example.com".into(),
            subject: "Hỏi".into(),
            preview: String::new(),
            body_plain: "Câu hỏi?".into(),
            received_at: String::new(),
            unread: false,
            starred: false,
            attachments: Vec::new(),
            thread_id: String::new(),
            to_line: String::new(),
            received_at_full: "1/1/2026".into(),
            labels: Vec::new(),
        }
    }

    #[test]
    fn signature_follows_the_from_identity_without_duplicating() {
        let p = prefs();
        let mut d = Draft::new("work");
        let body = sync_signature_in(&d.body, &d, &p);
        assert_eq!(body, "\n\n-- \nWork Linh");
        assert_eq!(
            sync_signature_in(&body, &d, &p),
            body,
            "second run changes nothing"
        );
        d.body = body;
        d.from_account = "home".into();
        let swapped = sync_signature_in(&d.body, &d, &p);
        assert_eq!(swapped, "\n\n-- \nHome Linh");
        assert!(!swapped.contains("Work"));
    }

    #[test]
    fn an_identity_without_a_signature_removes_the_old_one() {
        let p = prefs();
        let mut d = Draft::new("work");
        d.body = sync_signature_in("Chào", &d, &p);
        d.from_account = "other".into();
        assert_eq!(sync_signature_in(&d.body, &d, &p), "Chào");
    }

    #[test]
    fn replies_keep_the_quote_untouched_and_honour_use_for_replies() {
        let p = prefs();
        let mut d = Draft::reply_to(&message(), ReplyKind::Reply);
        let quote_before = d.body.clone();
        let with = sync_signature_in(&d.body, &d, &p);
        assert!(with.find("Work Linh").unwrap() < with.find("Câu hỏi?").unwrap());
        assert!(
            with.ends_with(quote_before.trim_start_matches('\n')),
            "quoted text stays byte-identical"
        );
        assert_eq!(with.matches("Work Linh").count(), 1);
        d.body = with.clone();
        assert_eq!(sync_signature_in(&with, &d, &p), with);
        // `home` has use_for_replies = false: switching to it drops the signature, quote intact.
        d.from_account = "home".into();
        let without = sync_signature_in(&d.body, &d, &p);
        assert!(!without.contains("Work Linh") && !without.contains("Home Linh"));
        assert!(without.contains("> Câu hỏi?"));
    }

    #[test]
    fn a_draft_with_only_the_automatic_signature_still_counts_as_empty() {
        let p = prefs();
        let known: Vec<&str> = p.signatures.values().map(|s| s.text.as_str()).collect();
        let mut d = Draft::new("work");
        assert!(d.is_empty_with(&known));
        d.body = sync_signature_in(&d.body, &d, &p);
        assert!(d.is_empty_with(&known));
        d.body.insert_str(0, "Xin chào");
        assert!(!d.is_empty_with(&known));
    }

    #[test]
    fn templates_fill_from_the_first_valid_recipient_and_keep_the_signature() {
        let p = prefs();
        let mut d = Draft::new("work");
        d.add_recipient(RecipientField::To, "Trần Linh <linh@x.vn>");
        let t = Template {
            id: "t".into(),
            name: "Cảm ơn".into(),
            subject: "Cảm ơn {{first_name}}".into(),
            body: "Chào {{name}} ({{email}})".into(),
        };
        let (subject, body) = apply_template_to(&d, &t, &p);
        assert_eq!(subject, "Cảm ơn Linh");
        assert_eq!(body, "Chào Trần Linh (linh@x.vn)\n\n-- \nWork Linh");
        // Applying again over its own result does not stack signatures.
        d.body = body;
        let (_, again) = apply_template_to(&d, &t, &p);
        assert_eq!(again.matches("Work Linh").count(), 1);
    }

    #[test]
    fn without_a_recipient_placeholders_stay_visible() {
        let d = Draft::new("work");
        let t = Template {
            id: "t".into(),
            name: "n".into(),
            subject: "Hi {{name}}".into(),
            body: "b".into(),
        };
        assert_eq!(
            apply_template_to(&d, &t, &Preferences::default()).0,
            "Hi {{name}}"
        );
    }
}
