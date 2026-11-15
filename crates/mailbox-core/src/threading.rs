//! Conversation threading (MB-201).
//!
//! Messages are linked by their `Message-ID`, `In-Reply-To` and `References` headers only, in the
//! spirit of the JWZ algorithm, and only inside one account. A subject-based fallback exists
//! solely for messages that carry no ids at all, and then only for the same normalized subject,
//! a short time window and at least one shared participant other than the account itself, so two
//! unrelated mails that happen to share a subject never merge.
//!
//! The thread id is derived from the root of the reference tree (the oldest id the messages point
//! at), so it stays the same while replies arrive and does not depend on which messages happen to
//! be cached.
use crate::MailboxError;
use crate::search::fold;
use std::collections::{HashMap, HashSet};

/// At most this many ids are taken from one `References` header (the newest ones).
pub const MAX_REFERENCES: usize = 100;
/// Longest accepted single message id, in bytes (RFC 5322 line limit).
pub const MAX_ID_LEN: usize = 998;
/// The subject fallback only joins messages at most this far apart.
pub const SUBJECT_WINDOW_SECS: i64 = 3 * 86_400;

/// Threading-relevant headers of one envelope, as fetched from the server.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EnvelopeHeaders {
    pub in_reply_to: Option<String>,
    /// Message ids, oldest first, each in `<...>` form.
    pub references: Vec<String>,
    /// Display text of the `To` and `Cc` headers (decoded, single line, bounded).
    pub to: String,
    pub cc: String,
}

/// Stored headers of one cached message (one row per message in the cache).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderRow {
    /// Cache id of the message (`MessageRecord::id`).
    pub message_id: String,
    pub account_id: String,
    pub headers: EnvelopeHeaders,
}

/// What the threader needs to know about one cached message.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ThreadMessage {
    /// Cache id of the message.
    pub key: String,
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub subject: String,
    pub date: i64,
    /// Lower-case addresses of sender and recipients.
    pub participants: Vec<String>,
}

/// All `<id>` tokens of a header value, in order, deduplicated, bounded. A bare token without
/// angle brackets is accepted when it looks like `local@domain`.
pub fn parse_msg_ids(value: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = value;
    let mut found_bracket = false;
    while let Some(open) = rest.find('<') {
        let Some(close) = rest[open..].find('>') else {
            break;
        };
        found_bracket = true;
        let inner = rest[open + 1..open + close].trim();
        if let Some(id) = normalize_id(inner)
            && !out.contains(&id)
        {
            out.push(id);
        }
        rest = &rest[open + close + 1..];
    }
    if !found_bracket {
        for token in value.split_whitespace() {
            if let Some(id) = normalize_id(token)
                && !out.contains(&id)
            {
                out.push(id);
            }
        }
    }
    if out.len() > MAX_REFERENCES {
        out.drain(..out.len() - MAX_REFERENCES);
    }
    out
}

/// `<local@domain>` form, or `None` for something that cannot be a message id.
pub fn normalize_id(inner: &str) -> Option<String> {
    let inner = inner.trim().trim_start_matches('<').trim_end_matches('>');
    let valid = !inner.is_empty()
        && inner.len() <= MAX_ID_LEN
        && inner.contains('@')
        && !inner
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '<' | '>'));
    valid.then(|| format!("<{inner}>"))
}

/// Subject without reply/forward prefixes and list tags, folded, for comparison only.
pub fn normalize_subject(subject: &str) -> String {
    const PREFIXES: [&str; 11] = [
        "re",
        "fw",
        "fwd",
        "aw",
        "wg",
        "sv",
        "vs",
        "tr",
        "rv",
        "tra loi",
        "chuyen tiep",
    ];
    let mut s = fold(subject);
    loop {
        let trimmed = s.trim_start();
        // `[list-name] ` tags in front.
        if let Some(rest) = trimmed.strip_prefix('[')
            && let Some(end) = rest.find(']')
        {
            s = rest[end + 1..].to_string();
            continue;
        }
        let mut stripped = None;
        for p in PREFIXES {
            if let Some(rest) = trimmed.strip_prefix(p) {
                // Optional counter: `Re[2]:` / `Re(2):`.
                let rest = rest.trim_start();
                let rest = match rest.chars().next() {
                    Some('[') | Some('(') => rest
                        .find([']', ')'])
                        .map(|e| &rest[e + 1..])
                        .unwrap_or(rest),
                    _ => rest,
                };
                if let Some(after) = rest.trim_start().strip_prefix(':') {
                    stripped = Some(after.to_string());
                    break;
                }
            }
        }
        match stripped {
            Some(rest) => s = rest,
            None => break,
        }
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 64-bit FNV-1a: small, stable across builds and platforms (not for security).
fn fnv64(text: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
        }
    }
    fn find(&mut self, mut x: usize) -> usize {
        while self.parent[x] != x {
            self.parent[x] = self.parent[self.parent[x]];
            x = self.parent[x];
        }
        x
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.parent[rb] = ra;
        }
    }
}

/// Group the messages of ONE account into conversations. Returns `(key, thread_id)` for every
/// input message. `account_id` prefixes the thread id so ids never collide across accounts;
/// `self_addresses` are the account's own addresses, ignored when comparing participants.
pub fn thread_messages(
    account_id: &str,
    messages: &[ThreadMessage],
    self_addresses: &[String],
) -> Vec<(String, String)> {
    // One container per distinct message id (a message cached in two folders shares one), plus
    // one per message that has no id of its own.
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut names: Vec<String> = Vec::new();
    let mut container = |id: &str, names: &mut Vec<String>| -> usize {
        if let Some(&i) = index.get(id) {
            return i;
        }
        let i = names.len();
        names.push(id.to_string());
        index.insert(id.to_string(), i);
        i
    };
    let mut own: Vec<usize> = Vec::with_capacity(messages.len());
    let mut chains: Vec<Vec<usize>> = Vec::with_capacity(messages.len());
    for m in messages {
        let me = match m.message_id.as_deref().and_then(normalize_id) {
            Some(id) => container(&id, &mut names),
            // NUL cannot appear in a real id, so synthetic names never collide with one.
            None => container(&format!("\0{}", m.key), &mut names),
        };
        own.push(me);
        let mut refs: Vec<String> = m
            .references
            .iter()
            .filter_map(|r| normalize_id(r))
            .collect();
        if let Some(parent) = m.in_reply_to.as_deref().and_then(normalize_id)
            && refs.last() != Some(&parent)
        {
            refs.retain(|r| r != &parent);
            refs.push(parent);
        }
        if refs.len() > MAX_REFERENCES {
            refs.drain(..refs.len() - MAX_REFERENCES);
        }
        chains.push(refs.iter().map(|r| container(r, &mut names)).collect());
    }

    let n = names.len();
    let mut uf = UnionFind::new(n);
    // Parent links (JWZ step 1), used only to find a stable root. A link that would close a
    // cycle is skipped; grouping uses union-find and is unaffected by cycles.
    let mut parent: Vec<Option<usize>> = vec![None; n];
    let creates_cycle = |parent: &Vec<Option<usize>>, child: usize, new_parent: usize| {
        let mut cur = Some(new_parent);
        let mut steps = 0;
        while let Some(c) = cur {
            if c == child || steps > n {
                return true;
            }
            cur = parent[c];
            steps += 1;
        }
        false
    };
    for (i, chain) in chains.iter().enumerate() {
        for w in chain.windows(2) {
            uf.union(w[0], w[1]);
            if w[0] != w[1] && parent[w[1]].is_none() && !creates_cycle(&parent, w[1], w[0]) {
                parent[w[1]] = Some(w[0]);
            }
        }
        if let Some(&last) = chain.last() {
            let me = own[i];
            uf.union(last, me);
            // The message's own header is authoritative for its own parent.
            if last != me && !creates_cycle(&parent, me, last) {
                parent[me] = Some(last);
            }
        }
    }

    // Subject fallback for messages without any id.
    let has_ids = |m: &ThreadMessage| {
        m.message_id.as_deref().and_then(normalize_id).is_some()
            || m.in_reply_to.as_deref().and_then(normalize_id).is_some()
            || m.references.iter().any(|r| normalize_id(r).is_some())
    };
    let me_set: HashSet<String> = self_addresses.iter().map(|a| a.to_lowercase()).collect();
    let others = |m: &ThreadMessage| -> HashSet<String> {
        m.participants
            .iter()
            .map(|p| p.to_lowercase())
            .filter(|p| !p.is_empty() && !me_set.contains(p))
            .collect()
    };
    let mut by_subject: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, m) in messages.iter().enumerate() {
        let s = normalize_subject(&m.subject);
        if !s.is_empty() {
            by_subject.entry(s).or_default().push(i);
        }
    }
    for group in by_subject.values() {
        if group.len() < 2 {
            continue;
        }
        for &i in group {
            if has_ids(&messages[i]) {
                continue;
            }
            let mine = others(&messages[i]);
            if mine.is_empty() {
                continue;
            }
            for &j in group {
                if i == j || (messages[i].date - messages[j].date).abs() > SUBJECT_WINDOW_SECS {
                    continue;
                }
                if others(&messages[j]).iter().any(|p| mine.contains(p)) {
                    uf.union(own[j], own[i]);
                }
            }
        }
    }

    // Root of each component: its parentless containers, preferring a real id, then the
    // smallest name, so the choice does not depend on input order.
    let mut root_of: HashMap<usize, usize> = HashMap::new();
    for (c, up) in parent.iter().enumerate() {
        if up.is_some() {
            continue;
        }
        let comp = uf.find(c);
        let better = |a: usize, b: usize| {
            let synthetic = |x: usize| names[x].starts_with('\0');
            (synthetic(a), &names[a]) < (synthetic(b), &names[b])
        };
        root_of
            .entry(comp)
            .and_modify(|r| {
                if better(c, *r) {
                    *r = c;
                }
            })
            .or_insert(c);
    }
    messages
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let comp = uf.find(own[i]);
            let root = root_of.get(&comp).copied().unwrap_or(comp);
            (
                m.key.clone(),
                format!("{account_id}:{:016x}", fnv64(&names[root])),
            )
        })
        .collect()
}

/// Storage port for headers and thread ids. Implemented by `mailbox-store`.
#[allow(async_fn_in_trait)]
pub trait ThreadStore {
    /// Everything the threader needs for one account.
    async fn thread_inputs(
        &self,
        owner_id: &str,
        account_id: &str,
    ) -> Result<Vec<ThreadMessage>, MailboxError>;
    /// Write `(message key, thread id)` pairs; only changed rows are touched. Returns the
    /// number of rows changed.
    async fn set_thread_ids(
        &self,
        owner_id: &str,
        account_id: &str,
        ids: &[(String, String)],
    ) -> Result<usize, MailboxError>;
    /// `(message key, thread id)` of every message of the account that has one.
    async fn thread_ids(
        &self,
        owner_id: &str,
        account_id: &str,
    ) -> Result<Vec<(String, String)>, MailboxError>;
    /// `(thread id, distinct messages)`; a message cached in two folders counts once.
    async fn thread_sizes(
        &self,
        owner_id: &str,
        account_id: &str,
    ) -> Result<Vec<(String, usize)>, MailboxError>;
    /// Every cached message of one conversation, oldest first.
    async fn conversation(
        &self,
        owner_id: &str,
        account_id: &str,
        thread_id: &str,
    ) -> Result<Vec<crate::store::MessageRecord>, MailboxError>;
    /// Stored headers of one message (`None` when not fetched yet).
    async fn headers_of(
        &self,
        owner_id: &str,
        message_key: &str,
    ) -> Result<Option<EnvelopeHeaders>, MailboxError>;
    /// UIDs of the folder's cached messages (current UIDVALIDITY) that have no stored headers.
    async fn uids_missing_headers(
        &self,
        owner_id: &str,
        account_id: &str,
        folder_id: &str,
        uid_validity: u64,
        limit: u32,
    ) -> Result<Vec<u32>, MailboxError>;
}

/// Recompute the conversations of one account and store the thread ids that changed.
pub async fn rethread_account(
    store: &impl ThreadStore,
    owner_id: &str,
    account_id: &str,
    self_addresses: &[String],
) -> Result<usize, MailboxError> {
    let inputs = store.thread_inputs(owner_id, account_id).await?;
    let ids = thread_messages(account_id, &inputs, self_addresses);
    store.set_thread_ids(owner_id, account_id, &ids).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(key: &str, id: Option<&str>, irt: Option<&str>, refs: &[&str]) -> ThreadMessage {
        ThreadMessage {
            key: key.into(),
            message_id: id.map(str::to_string),
            in_reply_to: irt.map(str::to_string),
            references: refs.iter().map(|r| r.to_string()).collect(),
            subject: "Kế hoạch".into(),
            date: 1_000,
            participants: vec!["a@x.org".into(), "me@here.org".into()],
        }
    }

    fn groups(out: &[(String, String)]) -> Vec<Vec<String>> {
        let mut map: HashMap<&str, Vec<String>> = HashMap::new();
        for (k, t) in out {
            map.entry(t.as_str()).or_default().push(k.clone());
        }
        let mut g: Vec<Vec<String>> = map
            .into_values()
            .map(|mut v| {
                v.sort();
                v
            })
            .collect();
        g.sort();
        g
    }

    fn me() -> Vec<String> {
        vec!["me@here.org".into()]
    }

    #[test]
    fn ids_are_parsed_bounded_and_deduplicated() {
        assert_eq!(
            parse_msg_ids(" <a@x> <b@y>\r\n <a@x> junk <bad> "),
            vec!["<a@x>", "<b@y>"]
        );
        assert_eq!(parse_msg_ids("bare@id.org"), vec!["<bare@id.org>"]);
        assert!(parse_msg_ids("no id here").is_empty());
        let many: String = (0..300).map(|i| format!("<m{i}@x> ")).collect();
        let ids = parse_msg_ids(&many);
        assert_eq!(ids.len(), MAX_REFERENCES);
        assert_eq!(ids.last().unwrap(), "<m299@x>", "newest ids are kept");
        assert!(normalize_id(&format!("{}@x", "a".repeat(2000))).is_none());
    }

    #[test]
    fn subjects_lose_reply_prefixes_tags_and_diacritics() {
        assert_eq!(normalize_subject("Re: RE: Fwd: Kế hoạch Q4"), "ke hoach q4");
        assert_eq!(
            normalize_subject("[team] Re[2]: Kế hoạch Q4"),
            "ke hoach q4"
        );
        assert_eq!(normalize_subject("Trả lời: Kế hoạch Q4"), "ke hoach q4");
        assert_eq!(
            normalize_subject("Return of the jedi"),
            "return of the jedi"
        );
        assert_eq!(normalize_subject("Re:"), "");
    }

    #[test]
    fn a_reply_chain_is_one_conversation() {
        let out = thread_messages(
            "acc",
            &[
                msg("1", Some("<r@x>"), None, &[]),
                msg("2", Some("<a@x>"), Some("<r@x>"), &["<r@x>"]),
                msg("3", Some("<b@x>"), Some("<a@x>"), &["<r@x>", "<a@x>"]),
            ],
            &me(),
        );
        assert_eq!(groups(&out), vec![vec!["1", "2", "3"]]);
        assert!(out[0].1.starts_with("acc:"));
    }

    #[test]
    fn a_missing_parent_still_joins_siblings_and_keeps_the_root_id() {
        // The root <r@x> is not cached; both replies refer to it.
        let partial = thread_messages(
            "acc",
            &[
                msg("2", Some("<a@x>"), Some("<r@x>"), &["<r@x>"]),
                msg("3", Some("<b@x>"), None, &["<r@x>", "<a@x>"]),
            ],
            &me(),
        );
        assert_eq!(groups(&partial), vec![vec!["2", "3"]]);
        // When the root arrives later the thread id does not change.
        let full = thread_messages(
            "acc",
            &[
                msg("1", Some("<r@x>"), None, &[]),
                msg("2", Some("<a@x>"), Some("<r@x>"), &["<r@x>"]),
                msg("3", Some("<b@x>"), None, &["<r@x>", "<a@x>"]),
            ],
            &me(),
        );
        assert_eq!(partial[0].1, full[1].1, "stable thread id");
    }

    #[test]
    fn forks_stay_in_one_conversation() {
        let out = thread_messages(
            "acc",
            &[
                msg("1", Some("<r@x>"), None, &[]),
                msg("2", Some("<a@x>"), Some("<r@x>"), &[]),
                msg("3", Some("<b@x>"), Some("<r@x>"), &[]),
                msg("4", Some("<c@x>"), Some("<b@x>"), &[]),
            ],
            &me(),
        );
        assert_eq!(groups(&out), vec![vec!["1", "2", "3", "4"]]);
    }

    #[test]
    fn duplicate_ids_in_two_folders_are_one_message_of_one_conversation() {
        let out = thread_messages(
            "acc",
            &[
                msg("inbox#1", Some("<r@x>"), None, &[]),
                msg("sent#9", Some("<a@x>"), Some("<r@x>"), &[]),
                msg("inbox#2", Some("<a@x>"), Some("<r@x>"), &[]),
            ],
            &me(),
        );
        assert_eq!(groups(&out), vec![vec!["inbox#1", "inbox#2", "sent#9"]]);
    }

    #[test]
    fn same_subject_strangers_stay_apart() {
        // Different ids, same subject, same day: never merged.
        let mut a = msg("1", Some("<one@x>"), None, &[]);
        let mut b = msg("2", Some("<two@y>"), None, &[]);
        a.subject = "Hello".into();
        b.subject = "Re: Hello".into();
        assert_eq!(groups(&thread_messages("acc", &[a, b], &me())).len(), 2);
        // No ids at all, same subject, but the only shared participant is the account itself.
        let mut c = msg("3", None, None, &[]);
        let mut d = msg("4", None, None, &[]);
        c.participants = vec!["stranger1@x.org".into(), "me@here.org".into()];
        d.participants = vec!["stranger2@y.org".into(), "me@here.org".into()];
        assert_eq!(groups(&thread_messages("acc", &[c, d], &me())).len(), 2);
    }

    #[test]
    fn id_less_messages_join_only_within_the_window_with_a_shared_participant() {
        let mut c = msg("3", None, None, &[]);
        let mut d = msg("4", None, None, &[]);
        d.subject = "RE: Kế hoạch".into();
        assert_eq!(
            groups(&thread_messages("acc", &[c.clone(), d.clone()], &me())),
            vec![vec!["3", "4"]]
        );
        d.date = c.date + SUBJECT_WINDOW_SECS + 1;
        assert_eq!(
            groups(&thread_messages("acc", &[c.clone(), d], &me())).len(),
            2
        );
        c.subject = String::new();
        let mut e = msg("5", None, None, &[]);
        e.subject = String::new();
        assert_eq!(
            groups(&thread_messages("acc", &[c, e], &me())).len(),
            2,
            "empty subjects never merge"
        );
    }

    #[test]
    fn cycles_in_references_terminate_and_group() {
        let out = thread_messages(
            "acc",
            &[
                msg("1", Some("<a@x>"), None, &["<b@x>"]),
                msg("2", Some("<b@x>"), None, &["<a@x>"]),
                msg("3", Some("<c@x>"), Some("<c@x>"), &["<c@x>", "<c@x>"]),
            ],
            &me(),
        );
        assert_eq!(groups(&out), vec![vec!["1", "2"], vec!["3"]]);
    }

    #[test]
    fn input_order_does_not_change_thread_ids() {
        let mut list = vec![
            msg("1", Some("<r@x>"), None, &[]),
            msg("2", Some("<a@x>"), Some("<r@x>"), &[]),
            msg("3", Some("<z@x>"), None, &[]),
        ];
        let first = thread_messages("acc", &list, &me());
        list.reverse();
        let mut second = thread_messages("acc", &list, &me());
        second.reverse();
        assert_eq!(first, second);
    }

    #[test]
    fn a_thousand_message_thread_is_one_conversation() {
        let mut list = vec![msg("0", Some("<m0@x>"), None, &[])];
        for i in 1..1000 {
            let refs: Vec<String> = (0..i)
                .rev()
                .take(20)
                .rev()
                .map(|j| format!("<m{j}@x>"))
                .collect();
            let refs: Vec<&str> = refs.iter().map(String::as_str).collect();
            list.push(msg(
                &i.to_string(),
                Some(&format!("<m{i}@x>")),
                Some(&format!("<m{}@x>", i - 1)),
                &refs,
            ));
        }
        let started = std::time::Instant::now();
        let out = thread_messages("acc", &list, &me());
        assert!(started.elapsed().as_secs() < 2);
        let g = groups(&out);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].len(), 1000);
    }

    #[test]
    fn accounts_never_share_thread_ids() {
        let a = thread_messages("a", &[msg("1", Some("<r@x>"), None, &[])], &me());
        let b = thread_messages("b", &[msg("1", Some("<r@x>"), None, &[])], &me());
        assert_ne!(a[0].1, b[0].1);
    }
}
