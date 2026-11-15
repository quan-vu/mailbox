//! Shared, platform-neutral contracts. Only public/demo-safe fields appear here.
//! No passwords, OAuth refresh tokens or private keys may be serialized here.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountStatus {
    /// Illustrative fixture account: no protocol round-trip has ever happened.
    Demo,
    Connected,
    NeedsAttention,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountSummary {
    pub id: String,
    pub name: String,
    pub address: String,
    pub status: AccountStatus,
    pub unread: u32,
    pub color: String,
    pub last_synced_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentSummary {
    pub id: String,
    pub filename: String,
    pub size_bytes: u64,
    pub media_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageSummary {
    pub id: String,
    pub account_id: String,
    pub folder_id: String,
    pub from_name: String,
    pub from_address: String,
    pub subject: String,
    pub preview: String,
    pub body_plain: String,
    pub received_at: String,
    pub unread: bool,
    pub starred: bool,
    pub attachments: Vec<AttachmentSummary>,
    /// Conversation key. Empty means "own thread" (backward-compatible default).
    #[serde(default)]
    pub thread_id: String,
    /// Display-only recipient line, e.g. `Product Team <product@company.vn>`.
    #[serde(default)]
    pub to_line: String,
    /// Absolute, localized timestamp for detail views and tooltips.
    #[serde(default)]
    pub received_at_full: String,
    #[serde(default)]
    pub labels: Vec<String>,
}

impl MessageSummary {
    /// Thread key used for grouping; falls back to the message id.
    pub fn thread_key(&self) -> &str {
        if self.thread_id.is_empty() {
            &self.id
        } else {
            &self.thread_id
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BootstrapResponse {
    pub mode: String,
    pub accounts: Vec<AccountSummary>,
    pub messages: Vec<MessageSummary>,
}

/// Fixtures are deterministic and safe to use in screenshots and unit tests.
/// They are not evidence of server connectivity. Names, dates and addresses are illustrative.
pub mod fixtures {
    use super::*;

    const PDF: &str = "application/pdf";
    const XLSX: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";

    fn attachment(
        id: &str,
        filename: &str,
        size_bytes: u64,
        media_type: &str,
    ) -> AttachmentSummary {
        AttachmentSummary {
            id: id.into(),
            filename: filename.into(),
            size_bytes,
            media_type: media_type.into(),
        }
    }

    struct M<'a> {
        id: &'a str,
        thread: &'a str,
        account: &'a str,
        from: (&'a str, &'a str),
        subject: &'a str,
        preview: &'a str,
        body: &'a str,
        at: (&'a str, &'a str),
        unread: bool,
        starred: bool,
        attachments: Vec<AttachmentSummary>,
        labels: &'a [&'a str],
    }

    impl M<'_> {
        fn build(self) -> MessageSummary {
            MessageSummary {
                id: self.id.into(),
                account_id: self.account.into(),
                folder_id: "inbox".into(),
                from_name: self.from.0.into(),
                from_address: self.from.1.into(),
                subject: self.subject.into(),
                preview: self.preview.into(),
                body_plain: self.body.into(),
                received_at: self.at.0.into(),
                unread: self.unread,
                starred: self.starred,
                attachments: self.attachments,
                thread_id: self.thread.into(),
                to_line: "Product Team <product@company.vn>".into(),
                received_at_full: self.at.1.into(),
                labels: self.labels.iter().map(|l| (*l).to_string()).collect(),
            }
        }
    }

    fn account(id: &str, name: &str, address: &str, unread: u32, color: &str) -> AccountSummary {
        AccountSummary {
            id: id.into(),
            name: name.into(),
            address: address.into(),
            status: AccountStatus::Demo,
            unread,
            color: color.into(),
            last_synced_at: None,
        }
    }

    const PLAN_BODY: &str = "Chào team,\n\nMình gửi kèm kế hoạch chi tiết cho việc ra mắt phiên bản 2.0 của sản phẩm MailBox, bao gồm timeline, phạm vi tính năng và các đầu công việc chính.\n\nMột số điểm chính:\n• Thời gian dự kiến ra mắt: 15/02\n• Tính năng trọng tâm: quản lý đa tài khoản, tìm kiếm nâng cao, cải thiện hiệu suất\n• Hiện trạng: đã hoàn thành ~80% các hạng mục chính\n\nNhờ mọi người xem qua và cho ý kiến trước thứ Sáu nhé.\n\nCảm ơn cả team!\nLinh";

    pub fn bootstrap() -> BootstrapResponse {
        let messages = vec![
            M {
                id: "m01",
                thread: "t01",
                account: "work",
                from: ("Trần Linh (Product)", "linh.tran@company.vn"),
                subject: "Kế hoạch ra mắt phiên bản 2.0",
                preview: "Chào team, mình gửi bản kế hoạch chi tiết cho việc ra mắt...",
                body: PLAN_BODY,
                at: ("14:20", "Thứ Tư, 11/11/2026 14:20"),
                unread: true,
                starred: true,
                attachments: vec![
                    attachment("a1", "Kế hoạch ra mắt v2.0.pdf", 3_355_443, PDF),
                    attachment("a2", "Timeline_v2.0.xlsx", 1_677_721, XLSX),
                ],
                labels: &["Sản phẩm"],
            },
            M {
                id: "m02",
                thread: "",
                account: "work",
                from: ("Thông báo nhóm", "updates@example.com"),
                subject: "Có 3 thông báo mới trong #product",
                preview: "Đức: Đã cập nhật tài liệu PRD. Mời mọi người xem và góp ý...",
                body: "Đã có ba thông báo mới trong kênh #product.",
                at: ("13:47", "Thứ Sáu, 13/11/2026 13:47"),
                unread: true,
                starred: false,
                attachments: vec![],
                labels: &[],
            },
            M {
                id: "m03",
                thread: "",
                account: "work",
                from: ("Nguyễn Văn Đức", "duc.nguyen@company.vn"),
                subject: "Báo cáo tiến độ sprint tuần 42",
                preview: "Đính kèm báo cáo tổng hợp công việc tuần này, bao gồm...",
                body: "Mình gửi báo cáo tiến độ sprint tuần này để team xem và phản hồi.",
                at: ("11:30", "Thứ Sáu, 13/11/2026 11:30"),
                unread: true,
                starred: false,
                attachments: vec![attachment("a3", "Sprint-42.pdf", 845_000, PDF)],
                labels: &[],
            },
            M {
                id: "m04",
                thread: "",
                account: "work",
                from: ("Kế toán", "ketoan@company.vn"),
                subject: "Hóa đơn dịch vụ tháng 11/2026",
                preview: "Gửi anh/chị hóa đơn dịch vụ (mã HĐ: INV-2611-083) vào...",
                body: "Chào anh/chị,\n\nĐính kèm là hóa đơn dịch vụ tháng này.\n\nTrân trọng.",
                at: ("10:15", "Thứ Sáu, 13/11/2026 10:15"),
                unread: true,
                starred: false,
                attachments: vec![attachment("a4", "INV-2611-083.pdf", 220_000, PDF)],
                labels: &[],
            },
            M {
                id: "m05",
                thread: "",
                account: "personal",
                from: ("Lan Anh (HR)", "hr@example.com"),
                subject: "Thông báo: Lịch nghỉ cuối năm",
                preview: "Công ty thông báo lịch nghỉ cuối năm từ 31/12 đến...",
                body: "Thông báo lịch nghỉ cuối năm, vui lòng kiểm tra và sắp xếp công việc.",
                at: ("09:28", "Thứ Sáu, 13/11/2026 09:28"),
                unread: true,
                starred: false,
                attachments: vec![],
                labels: &[],
            },
            M {
                id: "m06",
                thread: "",
                account: "personal",
                from: ("Hệ thống bảo mật", "security@example.com"),
                subject: "Cảnh báo bảo mật cho tài khoản",
                preview: "Đã có hoạt động đăng nhập mới vào tài khoản của bạn...",
                body: "Đã phát hiện một phiên đăng nhập mới. Hãy kiểm tra hoạt động gần đây.",
                at: ("08:41", "Thứ Sáu, 13/11/2026 08:41"),
                unread: false,
                starred: false,
                attachments: vec![],
                labels: &[],
            },
            M {
                id: "m07",
                thread: "",
                account: "support",
                from: ("Khách hàng - GreenTech", "contact@greentech.example"),
                subject: "Phản hồi về đề xuất hợp tác",
                preview: "Cảm ơn team đã gửi proposal. Bên mình rất quan tâm...",
                body: "Cảm ơn team đã gửi đề xuất hợp tác. Chúng tôi sẽ trao đổi nội bộ và phản hồi.",
                at: ("Hôm qua", "Thứ Năm, 12/11/2026 16:05"),
                unread: false,
                starred: true,
                attachments: vec![attachment("a5", "Proposal-GreenTech.pdf", 1_200_000, PDF)],
                labels: &[],
            },
            M {
                id: "m08",
                thread: "",
                account: "work",
                from: ("Lịch làm việc", "calendar@company.vn"),
                subject: "Nhắc nhở: Họp sản phẩm định kỳ",
                preview: "Cuộc họp sẽ diễn ra lúc 15:00 hôm nay qua họp trực tuyến.",
                body: "Nhắc nhở cuộc họp sản phẩm định kỳ lúc 15:00.",
                at: ("Hôm qua", "Thứ Năm, 12/11/2026 09:00"),
                unread: false,
                starred: false,
                attachments: vec![],
                labels: &[],
            },
            M {
                id: "m09",
                thread: "",
                account: "support",
                from: ("Mai Hương", "huong.mai@company.vn"),
                subject: "Tài liệu thiết kế landing page",
                preview: "Mình đã cập nhật bản thiết kế mới nhất, nhờ bạn xem và...",
                body: "Mình đã cập nhật bản thiết kế landing page mới nhất, nhờ bạn xem và góp ý.",
                at: ("Hôm qua", "Thứ Năm, 12/11/2026 08:12"),
                unread: false,
                starred: false,
                attachments: vec![attachment("a6", "Landing-v3.pdf", 2_400_000, PDF)],
                labels: &[],
            },
            // Replies in conversation t01, all received by the "work" account.
            M {
                id: "m10",
                thread: "t01",
                account: "work",
                from: ("Nguyễn Văn Đức", "duc.nguyen@company.vn"),
                subject: "Re: Kế hoạch ra mắt phiên bản 2.0",
                preview: "Mình đã xem và không có vấn đề lớn. Về phần phân công, đề xuất bổ sung thêm 1 đầu việc kiểm thử hiệu năng...",
                body: "Mình đã xem và không có vấn đề lớn. Về phần phân công, đề xuất bổ sung thêm 1 đầu việc kiểm thử hiệu năng.",
                at: ("16:08", "Thứ Tư, 11/11/2026 16:08"),
                unread: false,
                starred: false,
                attachments: vec![],
                labels: &[],
            },
            M {
                id: "m11",
                thread: "t01",
                account: "work",
                from: ("Mai Hương", "huong.mai@company.vn"),
                subject: "Re: Kế hoạch ra mắt phiên bản 2.0",
                preview: "Mình đã cập nhật lại timeline theo góp ý của Đức và đính kèm file mới.",
                body: "Mình đã cập nhật lại timeline theo góp ý của Đức (thêm kiểm thử hiệu năng) và đính kèm file mới.",
                at: ("09:32", "Thứ Năm, 12/11/2026 09:32"),
                unread: false,
                starred: false,
                attachments: vec![attachment(
                    "a7",
                    "Timeline_v2.0 (cập nhật).xlsx",
                    1_782_579,
                    XLSX,
                )],
                labels: &[],
            },
            M {
                id: "m12",
                thread: "t01",
                account: "work",
                from: ("Kế toán", "ketoan@company.vn"),
                subject: "Re: Kế hoạch ra mắt phiên bản 2.0",
                preview: "Về ngân sách, mình đã rà soát và xác nhận đủ nguồn lực cho các hạng mục.",
                body: "Về ngân sách, mình đã rà soát và xác nhận đủ nguồn lực cho các hạng mục trong kế hoạch.",
                at: ("10:24", "Thứ Sáu, 13/11/2026 10:24"),
                unread: false,
                starred: false,
                attachments: vec![],
                labels: &[],
            },
            M {
                id: "m13",
                thread: "t01",
                account: "work",
                from: ("Trần Linh (Product)", "linh.tran@company.vn"),
                subject: "Re: Kế hoạch ra mắt phiên bản 2.0",
                preview: "Cảm ơn mọi người! Mình đã cập nhật kế hoạch (v2) theo tất cả góp ý.",
                body: "Cảm ơn mọi người! Mình đã cập nhật kế hoạch (v2) theo tất cả góp ý. Phiên bản cuối ở đây nhé.",
                at: ("16:47", "Thứ Sáu, 13/11/2026 16:47"),
                unread: false,
                starred: false,
                attachments: vec![attachment(
                    "a8",
                    "Kế hoạch v2.0 (final).pdf",
                    3_565_158,
                    PDF,
                )],
                labels: &[],
            },
        ];
        BootstrapResponse {
            mode: "demo".into(),
            accounts: vec![
                account("work", "Work", "minh@company.vn", 6, "#2672F6"),
                account(
                    "personal",
                    "Personal",
                    "minh.nguyen@example.com",
                    4,
                    "#26B55C",
                ),
                account("support", "Support", "support@company.vn", 2, "#8B5CF6"),
            ],
            messages: messages.into_iter().map(M::build).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_mock_messages_reference_existing_accounts() {
        let data = fixtures::bootstrap();
        assert_eq!(data.mode, "demo");
        assert!(
            data.messages
                .iter()
                .all(|m| data.accounts.iter().any(|a| a.id == m.account_id))
        );
    }
    #[test]
    fn thread_replies_stay_in_their_receiving_account() {
        let data = fixtures::bootstrap();
        let thread: Vec<_> = data
            .messages
            .iter()
            .filter(|m| m.thread_key() == "t01")
            .collect();
        assert_eq!(thread.len(), 5);
        assert!(thread.iter().all(|m| m.account_id == "work"));
    }
    #[test]
    fn demo_accounts_never_claim_connected() {
        let data = fixtures::bootstrap();
        assert!(
            data.accounts
                .iter()
                .all(|a| a.status == AccountStatus::Demo)
        );
    }
    #[test]
    fn old_payload_without_new_fields_still_deserializes() {
        let json = r#"{"id":"x","account_id":"work","folder_id":"inbox","from_name":"A","from_address":"a@example.com","subject":"S","preview":"P","body_plain":"B","received_at":"1","unread":false,"starred":false,"attachments":[]}"#;
        let m: MessageSummary = serde_json::from_str(json).expect("backward compatible payload");
        assert_eq!(m.thread_key(), "x");
    }
    #[test]
    fn contracts_do_not_expose_secrets() {
        let serialized = serde_json::to_string(&fixtures::bootstrap()).expect("serialize");
        for forbidden in ["password", "refresh_token", "secret_key", "smtp_password"] {
            assert!(!serialized.contains(forbidden));
        }
    }
}
