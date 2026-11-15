//! English for strings added after the first catalog (scheduled send, auto-sync, audit fixes).
pub const ENTRIES: &[(&str, &str)] = &[
    (
        "Chuyển tiếp từ thư gốc — tải lại từ máy chủ khi gửi",
        "Forwarded from the original — fetched from the server again when sending",
    ),
    (
        "Đã quá giờ hẹn hơn 12 giờ trong lúc MailBox đóng hoặc máy ngủ, nên thư chưa được gửi. Bấm Thử lại để gửi ngay, hoặc Sửa để hẹn giờ khác.",
        "The scheduled time passed by more than 12 hours while MailBox was closed or the computer slept, so the message was not sent. Press Retry to send now, or Edit to pick another time.",
    ),
    ("Đã hẹn giờ", "Scheduled"),
    (
        "Máy chủ do tự động phát hiện trỏ tới một địa chỉ nội bộ (mạng riêng hoặc máy này) nên MailBox không kết nối. Nếu đây đúng là máy chủ trong mạng của bạn, hãy nhập tay tên máy chủ.",
        "The server found by auto-detection points to an internal address (a private network or this computer), so MailBox does not connect. If this really is a server on your network, type the server name by hand.",
    ),
    (
        "Không đọc được múi giờ của máy: giờ được tính theo UTC.",
        "This computer's time zone could not be read: times are in UTC.",
    ),
    (
        "Bản demo không gửi thư nên không hẹn giờ được",
        "The demo sends no mail, so nothing can be scheduled",
    ),
    ("Hẹn giờ gửi (demo)", "Schedule send (demo)"),
    ("Hẹn giờ gửi", "Schedule send"),
    ("Gửi thư này vào lúc", "Send this message at"),
    ("Sau 1 giờ", "In 1 hour"),
    ("8:00 sáng mai", "Tomorrow 8:00 am"),
    ("Thời điểm gửi", "Send time"),
    ("Thôi", "Cancel"),
    ("Chọn ngày và giờ gửi.", "Pick a send date and time."),
    ("Hẹn giờ", "Schedule"),
    ("thời điểm đã chọn", "the chosen time"),
    (
        "Thư đang chờ trong hộp thư đi và nội dung được khóa. Muốn sửa, hãy hủy hẹn giờ trước.",
        "The message waits in the outbox and its content is locked. To edit it, cancel the schedule first.",
    ),
    (
        "Không hủy được: thư đã bắt đầu gửi.",
        "Cannot cancel: sending has already started.",
    ),
    ("Hủy hẹn giờ để sửa", "Cancel schedule to edit"),
    (
        "Thư đã bắt đầu gửi theo giờ hẹn.",
        "The message has already started sending at its scheduled time.",
    ),
    ("Gửi ngay", "Send now"),
    ("Hủy hẹn giờ", "Cancel schedule"),
    (
        "Giữ kết nối tới máy chủ khi MailBox đang mở: nhận thư ngay nếu máy chủ hỗ trợ IDLE, nếu không thì kiểm tra mỗi 2 phút.",
        "Keeps a connection to the server while MailBox is open: mail arrives at once if the server supports IDLE, otherwise MailBox checks every 2 minutes.",
    ),
    (
        "Đồng bộ tự động đang tắt: MailBox không giữ kết nối và không tự kiểm tra thư mới. Bấm Đồng bộ để lấy thư khi cần. Bấm Lưu thay đổi để giữ lựa chọn này sau khi mở lại.",
        "Automatic sync is off: MailBox holds no connection and does not check for new mail by itself. Press Sync to fetch mail when needed. Press Save changes to keep this choice after reopening.",
    ),
    (
        "Chọn thời điểm ít nhất 1 phút sau bây giờ.",
        "Pick a time at least 1 minute from now.",
    ),
    (
        "Chỉ hẹn giờ được trong vòng một năm.",
        "Scheduling is limited to one year ahead.",
    ),
    (
        "Có thư hẹn giờ đã quá hạn hơn 12 giờ nên chưa được gửi. Mở Bản nháp để quyết định.",
        "A scheduled message is more than 12 hours overdue and was not sent. Open Drafts to decide.",
    ),
    ("Bản demo không gửi thư.", "The demo sends no mail."),
    (
        "Chưa lưu được bản nháp nên chưa hẹn giờ. Thử lại.",
        "The draft could not be saved, so nothing was scheduled. Try again.",
    ),
    (
        "Không hẹn giờ được: thư có thể đã bắt đầu gửi. Xem trạng thái bên dưới.",
        "Could not schedule: sending may already have started. See the state below.",
    ),
    ("Bản web không gửi thư.", "The web version sends no mail."),
    ("Đã gửi một thư hẹn giờ.", "A scheduled message was sent."),
    (
        "Thư hẹn giờ có thể đã gửi hoặc chưa (mất kết nối giữa chừng). Mở Bản nháp để kiểm tra; MailBox không tự gửi lại.",
        "A scheduled message may or may not have been sent (the connection dropped midway). Open Drafts to check; MailBox does not resend by itself.",
    ),
    (
        "Thư hẹn giờ chưa gửi được. Mở Bản nháp để xem lý do và thử lại.",
        "A scheduled message could not be sent. Open Drafts to see why and retry.",
    ),
    (
        "Không đọc được danh sách tệp đính kèm của thư gốc trên máy này; tệp đính kèm chưa được chuyển tiếp.",
        "The original message's attachment list could not be read on this device; attachments were not forwarded.",
    ),
    (
        "Nội dung thư gốc chưa được tải về máy này nên MailBox chưa biết thư có tệp đính kèm hay không: tệp đính kèm (nếu có) chưa được chuyển tiếp. Mở thư gốc để tải nội dung, rồi bấm Chuyển tiếp lại.",
        "The original message has not been downloaded to this device, so MailBox does not know whether it has attachments: attachments (if any) were not forwarded. Open the original message to download it, then press Forward again.",
    ),
];
