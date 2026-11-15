//! User preferences and message templates (MB-210, MB-211).
//!
//! Settings are one typed, versioned record per owner. They never hold a secret: no password,
//! token or key, and the remote-images default is forced to "blocked". Templates are separate
//! records so each can be edited on its own. Implementations of [`PreferencesStore`] live in
//! `mailbox-store`; every call is scoped by `owner_id`.
use crate::MailboxError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Schema version written with every record. A record with a higher version is refused instead
/// of being silently reinterpreted by an older build.
pub const PREFERENCES_VERSION: u32 = 1;
pub const MAX_TEMPLATES: usize = 100;
pub const MAX_TEMPLATE_NAME: usize = 80;
pub const MAX_TEMPLATE_SUBJECT: usize = 200;
pub const MAX_TEMPLATE_BODY: usize = 20_000;
pub const MAX_SIGNATURE: usize = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeChoice {
    #[default]
    Light,
    Dark,
    System,
}

/// Interface language (MB-105). Vietnamese is the default; a stored record without the field
/// keeps Vietnamese.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lang {
    #[default]
    Vi,
    En,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Density {
    #[default]
    Comfortable,
    Compact,
}

/// What a desktop notification may reveal. `CountOnly` is the default; `SenderAndSubject` is an
/// explicit opt-in. No mode ever shows the message body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotifyMode {
    Off,
    #[default]
    CountOnly,
    SenderAndSubject,
}

/// Plain-text signature of one account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signature {
    pub text: String,
    /// Also add it to replies and forwards.
    pub use_for_replies: bool,
}

impl Default for Signature {
    fn default() -> Self {
        Self {
            text: String::new(),
            use_for_replies: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub version: u32,
    pub theme: ThemeChoice,
    pub density: Density,
    pub lang: Lang,
    pub reduce_motion: bool,
    /// Account id used for new mail. `None` = first account.
    pub default_identity: Option<String>,
    /// Per account id.
    pub signatures: BTreeMap<String, Signature>,
    pub notify_mode: NotifyMode,
    pub notify_sound: bool,
    pub notify_sync_errors: bool,
    /// Always `false` by default. Loading a record cannot turn remote images on by accident:
    /// the field is kept for the explicit, later per-sender feature.
    pub remote_images: bool,
    pub shortcuts_enabled: bool,
    /// Live sync (MB-140): one held connection per account, IDLE or a 2-minute poll. Off: no
    /// background connection; the manual "sync" button still works. Records written before
    /// this field existed read as `true` (the behaviour they had).
    pub auto_sync: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            version: PREFERENCES_VERSION,
            theme: ThemeChoice::Light,
            density: Density::Comfortable,
            lang: Lang::Vi,
            reduce_motion: false,
            default_identity: None,
            signatures: BTreeMap::new(),
            notify_mode: NotifyMode::CountOnly,
            notify_sound: false,
            notify_sync_errors: true,
            remote_images: false,
            shortcuts_enabled: true,
            auto_sync: true,
        }
    }
}

impl Preferences {
    /// Clamp values to what the product allows. Run on every load and save.
    pub fn sanitized(mut self) -> Self {
        self.version = PREFERENCES_VERSION;
        self.remote_images = false;
        for sig in self.signatures.values_mut() {
            sig.text = sig
                .text
                .chars()
                .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
                .take(MAX_SIGNATURE)
                .collect();
        }
        self.signatures.retain(|id, _| !id.trim().is_empty());
        self
    }

    /// "Restore defaults": appearance, notifications, shortcuts and security go back to the
    /// factory values. Text the user wrote (signatures) and the chosen identity are kept.
    pub fn restore_defaults(&self) -> Self {
        Self {
            default_identity: self.default_identity.clone(),
            signatures: self.signatures.clone(),
            // The language is kept: resetting it would leave the user in a language they cannot read.
            lang: self.lang,
            ..Self::default()
        }
    }

    pub fn signature_for(&self, account_id: &str) -> Option<&Signature> {
        self.signatures
            .get(account_id)
            .filter(|s| !s.text.trim().is_empty())
    }
}

/// Decode a stored record. Empty input means "no record yet" and gives the defaults.
pub fn decode_preferences(json: Option<&str>) -> Result<Preferences, MailboxError> {
    let Some(json) = json else {
        return Ok(Preferences::default());
    };
    let prefs: Preferences =
        serde_json::from_str(json).map_err(|_| MailboxError::InconsistentData("preferences"))?;
    if prefs.version > PREFERENCES_VERSION {
        return Err(MailboxError::InconsistentData("preferences version"));
    }
    Ok(prefs.sanitized())
}

pub fn encode_preferences(prefs: &Preferences) -> Result<String, MailboxError> {
    serde_json::to_string(&prefs.clone().sanitized())
        .map_err(|_| MailboxError::InconsistentData("preferences"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    pub id: String,
    pub name: String,
    pub subject: String,
    pub body: String,
}

impl Template {
    /// Reason a template cannot be saved, if any (Vietnamese, shown to the user).
    pub fn problem(&self) -> Option<&'static str> {
        if self.id.trim().is_empty() {
            return Some("Mẫu thiếu mã định danh.");
        }
        if self.name.trim().is_empty() {
            return Some("Đặt tên cho mẫu.");
        }
        if self.name.chars().count() > MAX_TEMPLATE_NAME {
            return Some("Tên mẫu quá dài (tối đa 80 ký tự).");
        }
        if self.subject.chars().count() > MAX_TEMPLATE_SUBJECT {
            return Some("Chủ đề của mẫu quá dài (tối đa 200 ký tự).");
        }
        if self.subject.chars().any(char::is_control) {
            return Some("Chủ đề không được có xuống dòng.");
        }
        if self.body.chars().count() > MAX_TEMPLATE_BODY {
            return Some("Nội dung mẫu quá dài (tối đa 20.000 ký tự).");
        }
        None
    }
}

/// Values a template can use, taken from the first recipient of the draft.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TemplateContext {
    pub name: String,
    pub email: String,
}

impl TemplateContext {
    /// `display` may be empty; then the part of the address before `@` is used.
    pub fn from_recipient(display: &str, address: &str) -> Self {
        let name = display.trim();
        let name = if name.is_empty() || name == address {
            address.split('@').next().unwrap_or("")
        } else {
            name
        };
        Self {
            name: plain(name),
            email: plain(address),
        }
    }

    fn first_name(&self) -> String {
        self.name
            .split_whitespace()
            .last()
            .unwrap_or_default()
            .to_string()
    }
}

/// Inserted values are plain text: control characters become spaces and the value is never
/// searched for placeholders again, so a recipient named `{{email}}` stays literal.
fn plain(value: &str) -> String {
    value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

/// Fill `{{name}}`, `{{first_name}}` and `{{email}}` in one pass. Unknown placeholders and
/// placeholders without a value (no recipient yet) stay as typed, so the user sees them.
pub fn fill_template(text: &str, ctx: Option<&TemplateContext>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            out.push_str(&rest[start..]);
            return out;
        };
        let key = after[..end].trim();
        let value = ctx.and_then(|c| match key {
            "name" => Some(c.name.clone()),
            "first_name" => Some(c.first_name()),
            "email" => Some(c.email.clone()),
            _ => None,
        });
        match value.filter(|v| !v.is_empty()) {
            Some(v) => out.push_str(&v),
            None => out.push_str(&rest[start..start + 2 + end + 2]),
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

/// Subjects are one line: line breaks from a filled value must not survive.
pub fn fill_subject(text: &str, ctx: Option<&TemplateContext>) -> String {
    fill_template(text, ctx)
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

#[allow(async_fn_in_trait)]
pub trait PreferencesStore {
    /// The stored settings, or the defaults when nothing was saved yet.
    async fn load_preferences(&self, owner_id: &str) -> Result<Preferences, MailboxError>;
    async fn save_preferences(
        &self,
        owner_id: &str,
        prefs: &Preferences,
    ) -> Result<(), MailboxError>;
    /// Forget the stored settings record (back to defaults). Templates are not touched.
    async fn reset_preferences(&self, owner_id: &str) -> Result<(), MailboxError>;

    /// Ordered by name, then id.
    async fn list_templates(&self, owner_id: &str) -> Result<Vec<Template>, MailboxError>;
    async fn upsert_template(
        &self,
        owner_id: &str,
        template: &Template,
    ) -> Result<(), MailboxError>;
    async fn delete_template(
        &self,
        owner_id: &str,
        template_id: &str,
    ) -> Result<bool, MailboxError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> TemplateContext {
        TemplateContext::from_recipient("Trần Linh", "linh@x.vn")
    }

    #[test]
    fn placeholders_fill_from_first_recipient() {
        let t = "Chào {{first_name}} ({{ email }}), {{name}}!";
        assert_eq!(
            fill_template(t, Some(&ctx())),
            "Chào Linh (linh@x.vn), Trần Linh!"
        );
    }

    #[test]
    fn values_are_plain_text_and_not_expanded_again() {
        let evil = TemplateContext::from_recipient("{{email}}\r\nBcc: a@b.c", "x@y.vn");
        let out = fill_template("Hi {{name}} / {{email}}", Some(&evil));
        assert_eq!(out, "Hi {{email}}  Bcc: a@b.c / x@y.vn");
        assert!(!fill_subject("{{name}}", Some(&evil)).contains('\n'));
    }

    #[test]
    fn missing_values_and_unknown_keys_stay_visible() {
        assert_eq!(
            fill_template("Hi {{name}} {{other}}", None),
            "Hi {{name}} {{other}}"
        );
        assert_eq!(fill_template("open {{ name", Some(&ctx())), "open {{ name");
    }

    #[test]
    fn name_falls_back_to_the_local_part() {
        let c = TemplateContext::from_recipient("", "linh.tran@x.vn");
        assert_eq!(c.name, "linh.tran");
    }

    #[test]
    fn template_limits() {
        let ok = Template {
            id: "t".into(),
            name: "Cảm ơn".into(),
            subject: "Re".into(),
            body: "x".into(),
        };
        assert_eq!(ok.problem(), None);
        assert!(
            Template {
                name: " ".into(),
                ..ok.clone()
            }
            .problem()
            .is_some()
        );
        assert!(
            Template {
                subject: "a\nb".into(),
                ..ok.clone()
            }
            .problem()
            .is_some()
        );
        assert!(
            Template {
                body: "x".repeat(MAX_TEMPLATE_BODY + 1),
                ..ok
            }
            .problem()
            .is_some()
        );
    }

    #[test]
    fn remote_images_cannot_be_turned_on_by_a_stored_record() {
        let p = decode_preferences(Some(r#"{"remote_images":true,"theme":"dark"}"#)).unwrap();
        assert!(!p.remote_images);
        assert_eq!(p.theme, ThemeChoice::Dark);
    }

    #[test]
    fn newer_or_broken_records_are_refused_and_none_means_defaults() {
        assert_eq!(decode_preferences(None).unwrap(), Preferences::default());
        assert!(decode_preferences(Some(r#"{"version":99}"#)).is_err());
        assert!(decode_preferences(Some("not json")).is_err());
    }

    #[test]
    fn restore_defaults_keeps_user_text() {
        let mut p = Preferences {
            theme: ThemeChoice::Dark,
            notify_mode: NotifyMode::Off,
            ..Default::default()
        };
        p.default_identity = Some("acc-a".into());
        p.signatures.insert(
            "acc-a".into(),
            Signature {
                text: "Linh".into(),
                use_for_replies: false,
            },
        );
        let r = p.restore_defaults();
        assert_eq!(r.theme, ThemeChoice::Light);
        assert_eq!(r.notify_mode, NotifyMode::CountOnly);
        assert_eq!(r.default_identity.as_deref(), Some("acc-a"));
        assert_eq!(r.signatures["acc-a"].text, "Linh");
    }

    #[test]
    fn roundtrip_through_json() {
        let mut p = Preferences {
            density: Density::Compact,
            ..Preferences::default()
        };
        p.signatures.insert(
            "a".into(),
            Signature {
                text: "Hi\nthere\u{7}".into(),
                use_for_replies: true,
            },
        );
        let back = decode_preferences(Some(&encode_preferences(&p).unwrap())).unwrap();
        assert_eq!(back.density, Density::Compact);
        assert_eq!(back.signatures["a"].text, "Hi\nthere");
    }

    #[test]
    fn auto_sync_defaults_on_and_older_records_keep_it_on() {
        assert!(Preferences::default().auto_sync);
        let old = r#"{"version":1,"theme":"dark"}"#;
        assert!(decode_preferences(Some(old)).unwrap().auto_sync);
        let off = Preferences {
            auto_sync: false,
            ..Preferences::default()
        };
        let json = encode_preferences(&off).unwrap();
        assert!(!decode_preferences(Some(&json)).unwrap().auto_sync);
        assert!(
            off.restore_defaults().auto_sync,
            "restore defaults turns it back on"
        );
    }

    #[test]
    fn language_defaults_to_vietnamese_for_old_records() {
        let old = r#"{"version":1,"theme":"dark"}"#;
        assert_eq!(decode_preferences(Some(old)).unwrap().lang, Lang::Vi);
        assert_eq!(Preferences::default().lang, Lang::Vi);
    }

    #[test]
    fn language_roundtrips_and_survives_restore_defaults() {
        let p = Preferences {
            lang: Lang::En,
            ..Preferences::default()
        };
        let json = encode_preferences(&p).unwrap();
        assert!(json.contains("\"lang\":\"en\""));
        assert_eq!(decode_preferences(Some(&json)).unwrap().lang, Lang::En);
        assert_eq!(p.restore_defaults().lang, Lang::En);
    }
}
