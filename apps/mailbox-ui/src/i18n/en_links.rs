//! English for the link confirmation dialog (AUDIT S-09).
pub const ENTRIES: &[(&str, &str)] = &[
    ("Mở liên kết này?", "Open this link?"),
    (
        "Liên kết này nằm trong thư và sẽ mở bên ngoài MailBox. Hãy kiểm tra địa chỉ thật bên dưới: chữ hiển thị trong thư có thể khác.",
        "This link comes from the message and opens outside MailBox. Check the real address below: the text shown in the message can differ.",
    ),
    ("Trang web", "Website"),
    ("Người nhận", "Recipient"),
    ("Địa chỉ đầy đủ", "Full address"),
    (
        "Kết nối không mã hóa (http): người khác trên mạng có thể đọc hoặc sửa nội dung.",
        "Unencrypted connection (http): others on the network can read or change the content.",
    ),
    (
        "Tên miền này dùng ký tự quốc tế (dạng xn--), cách thường được dùng để bắt chước một tên quen thuộc.",
        "This domain uses international characters (xn-- form), a common way to imitate a familiar name.",
    ),
    (
        "Không mở được. Bạn có thể chép địa chỉ ở trên và tự mở.",
        "Could not open it. You can copy the address above and open it yourself.",
    ),
    ("Mở trong trình duyệt", "Open in browser"),
    ("Mở ứng dụng thư", "Open mail app"),
    (
        "MailBox không mở liên kết này",
        "MailBox does not open this link",
    ),
    (
        "Địa chỉ có tên đăng nhập đứng trước dấu @ của tên miền. Đây là cách thường dùng để giả mạo tên trang web, nên MailBox không mở.",
        "The address has a user name before the @ of the domain. This is a common way to fake a site name, so MailBox does not open it.",
    ),
    (
        "MailBox chỉ mở liên kết http, https và mailto. Liên kết này thuộc loại khác.",
        "MailBox only opens http, https and mailto links. This link is of another kind.",
    ),
    (
        "Địa chỉ của liên kết quá dài nên MailBox không mở.",
        "The link address is too long, so MailBox does not open it.",
    ),
    (
        "Địa chỉ của liên kết không hợp lệ.",
        "The link address is not valid.",
    ),
    ("Đóng", "Close"),
    (
        "Liên kết mẫu (demo) để thử hộp xác nhận trước khi mở:",
        "Sample links (demo) to try the confirmation shown before opening:",
    ),
];
