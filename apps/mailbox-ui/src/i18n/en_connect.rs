//! English catalog part: connect. Key = the Vietnamese text used in the code.
pub(super) static ENTRIES: &[(&str, &str)] = &[
    ("(Không có tiêu đề)", "(No subject)"),
    (
        "Tên máy chủ hoặc cổng không hợp lệ. Kiểm tra lại thông tin IMAP.",
        "The server name or port is not valid. Check the IMAP details.",
    ),
    (
        "Không kết nối được tới máy chủ. Kiểm tra tên máy chủ, cổng và kết nối mạng.",
        "Could not connect to the server. Check the server name, port and network connection.",
    ),
    (
        "Máy chủ không phản hồi kịp. Kiểm tra cổng IMAP (thường là 993) rồi thử lại.",
        "The server did not respond in time. Check the IMAP port (usually 993) and try again.",
    ),
    (
        "Chứng chỉ của máy chủ không hợp lệ cho tên máy chủ này. MailBox không cho phép bỏ qua — hãy dùng đúng tên máy chủ do nhà cung cấp hướng dẫn hoặc liên hệ họ.",
        "The server's certificate is not valid for this server name. MailBox does not allow bypassing it — use the exact server name your provider gives or contact them.",
    ),
    (
        "Không thiết lập được kết nối mã hóa với máy chủ. Kiểm tra cổng SSL/TLS (993).",
        "Could not set up an encrypted connection with the server. Check the SSL/TLS port (993).",
    ),
    (
        "Máy chủ này không trả lời như một máy chủ IMAP. Kiểm tra tên máy chủ và cổng.",
        "This server does not answer like an IMAP server. Check the server name and port.",
    ),
    (
        "Máy chủ không cho đăng nhập bằng mật khẩu (thường cần OAuth). Loại tài khoản này chưa được hỗ trợ.",
        "The server does not allow password sign-in (it usually needs OAuth). This account type is not supported yet.",
    ),
    (
        "Sai địa chỉ email hoặc mật khẩu. Nếu tài khoản bật xác thực 2 bước, hãy dùng mật khẩu ứng dụng.",
        "Wrong email address or password. If the account uses 2-step verification, use an app password.",
    ),
    (
        "Máy chủ này không trả lời như một máy chủ gửi thư (SMTP). Kiểm tra tên máy chủ và cổng (465 hoặc 587).",
        "This server does not answer like a mail-sending (SMTP) server. Check the server name and port (465 or 587).",
    ),
    (
        "Máy chủ không hỗ trợ mã hóa STARTTLS trên cổng này nên MailBox dừng lại, không gửi mật khẩu. Hãy dùng cổng 465 (SSL/TLS).",
        "The server does not support STARTTLS encryption on this port, so MailBox stopped and did not send the password. Use port 465 (SSL/TLS).",
    ),
    (
        "Máy chủ gửi thư không cho đăng nhập bằng mật khẩu (thường cần OAuth). Loại tài khoản này chưa được hỗ trợ.",
        "The mail-sending server does not allow password sign-in (it usually needs OAuth). This account type is not supported yet.",
    ),
    (
        "Không lưu được dữ liệu trên máy này (cơ sở dữ liệu cục bộ hoặc Keychain). Kiểm tra quyền truy cập rồi thử lại.",
        "Could not save data on this computer (local database or Keychain). Check access permissions and try again.",
    ),
    (
        "Bản này chưa hỗ trợ kiểm tra kết nối thật.",
        "This version cannot test real connections yet.",
    ),
    (
        "Không kết nối được máy chủ dữ liệu MailBox. Kiểm tra máy chủ đang chạy rồi thử lại.",
        "Could not connect to the MailBox data server. Check that the server is running and try again.",
    ),
    (
        "Máy chủ trả về dữ liệu không hợp lệ. Thử lại sau.",
        "The server returned invalid data. Try again later.",
    ),
    (
        "Chưa phát hiện được. Nhập thủ công.",
        "Could not detect it. Enter it manually.",
    ),
    ("Dùng đề xuất này", "Use this suggestion"),
    (
        "Gmail: bật xác minh 2 bước, tạo \"Mật khẩu ứng dụng\" trong tài khoản Google rồi dán vào ô mật khẩu.",
        "Gmail: turn on 2-step verification, create an \"App password\" in your Google account and paste it into the password field.",
    ),
    (
        "Microsoft 365 / Outlook.com: nhiều tổ chức đã tắt đăng nhập bằng mật khẩu IMAP. Nếu quản trị viên cho phép, dùng mật khẩu ứng dụng; nếu không, cần OAuth (chưa có trong bản này).",
        "Microsoft 365 / Outlook.com: many organizations have turned off IMAP password sign-in. If your administrator allows it, use an app password; otherwise OAuth is needed (not in this version).",
    ),
    ("mock / chưa hoạt động thật", "mock / not functional"),
    ("Mock / chưa hoạt động thật. ", "Mock / not functional. "),
    (
        "Cần gì để có bản thật: ",
        "What is needed for the real thing: ",
    ),
    ("Dùng được ngay: ", "Available now: "),
    (
        "đăng nhập bằng mật khẩu ứng dụng. ",
        "sign in with an app password. ",
    ),
    (
        "Nhập địa chỉ email bên dưới, bật \"Tự động phát hiện cấu hình\" để MailBox đề xuất máy chủ IMAP/SMTP, rồi kiểm tra kết nối.",
        "Enter your email address below, turn on \"Detect settings automatically\" so MailBox can suggest the IMAP/SMTP servers, then test the connection.",
    ),
    (
        "Đã hiểu, dùng mật khẩu ứng dụng",
        "Got it, use an app password",
    ),
    ("Giới thiệu", "Introduction"),
    (
        "Kết nối nhiều tài khoản email vào MailBox",
        "Connect several email accounts to MailBox",
    ),
    (
        "Tập trung hộp thư có sẵn — Gmail, Microsoft 365, Zoho hay email doanh nghiệp — trong một nơi. Máy chủ gốc vẫn là nơi lưu thư.",
        "Bring your existing mailboxes — Gmail, Microsoft 365, Zoho or business email — into one place. The original server is still where mail is stored.",
    ),
    ("Tập trung & đồng bộ", "Unified & synced"),
    (
        "Xem và xử lý thư nhiều tài khoản cùng lúc.",
        "View and handle mail from several accounts at once.",
    ),
    ("Làm việc hiệu quả hơn", "Work more efficiently"),
    (
        "Tìm kiếm, lọc, gắn sao trên một giao diện.",
        "Search, filter and star in one interface.",
    ),
    ("An toàn & bảo mật", "Safe & secure"),
    (
        "IMAP/SMTP qua TLS có kiểm tra chứng chỉ; mật khẩu không lưu dạng văn bản.",
        "IMAP/SMTP over TLS with certificate checks; passwords are not stored as plain text.",
    ),
    ("Bản Desktop: ", "Desktop version: "),
    (
        "kết nối thật qua TLS. Sau khi kiểm tra, bạn có thể lưu tài khoản và đồng bộ hộp thư đến (chỉ đọc).",
        "real connection over TLS. After the check you can save the account and sync the inbox (read-only).",
    ),
    ("Bản Web: ", "Web version: "),
    (
        "màn này không kết nối mạng và không nhận mật khẩu.",
        "this screen makes no network connection and takes no password.",
    ),
    ("Thêm tài khoản email", "Add an email account"),
    (
        "Kết nối một tài khoản email đã có để đưa hộp thư về MailBox.",
        "Connect an existing email account to bring its mailbox into MailBox.",
    ),
    ("Tiến trình", "Progress"),
    ("Nhập thông tin", "Enter details"),
    ("Kiểm tra kết nối", "Test connection"),
    ("Hoàn tất", "Done"),
    ("1. Chọn nhà cung cấp ", "1. Choose a provider "),
    ("(tùy chọn)", "(optional)"),
    ("Nhà cung cấp", "Provider"),
    (
        "Tự động phát hiện cấu hình",
        "Detect settings automatically",
    ),
    (
        "Bảng nhà cung cấp, DNS SRV, autoconfig qua HTTPS và tên máy chủ thông dụng. Chỉ tên miền được dùng; luôn cho phép sửa tay.",
        "Provider table, DNS SRV, autoconfig over HTTPS and common server names. Only the domain is used; you can always edit by hand.",
    ),
    (
        "Gợi ý host/cổng theo quy tắc nhà cung cấp đã biết; luôn cho phép sửa tay. Không tải URL tùy ý.",
        "Suggests host/port from known provider rules; you can always edit by hand. No arbitrary URL is fetched.",
    ),
    ("Đang dò cấu hình…", "Detecting settings…"),
    (
        "Dò cấu hình từ địa chỉ email",
        "Detect settings from the email address",
    ),
    (
        "Nhập email trước. Có thể mất vài giây.",
        "Enter the email first. It can take a few seconds.",
    ),
    ("2. Thông tin kết nối", "2. Connection details"),
    ("Địa chỉ email *", "Email address *"),
    (
        "Mật khẩu / Mật khẩu ứng dụng *",
        "Password / App password *",
    ),
    (
        "Nhập mật khẩu hộp thư hoặc mật khẩu ứng dụng",
        "Enter the mailbox password or an app password",
    ),
    (
        "Nhập mật khẩu để kiểm tra kết nối.",
        "Enter the password to test the connection.",
    ),
    (
        "Chỉ dùng để kiểm tra kết nối qua TLS. Không lưu, không ghi log.",
        "Used only to test the connection over TLS. Not stored, not logged.",
    ),
    ("Mật khẩu / Mật khẩu ứng dụng", "Password / App password"),
    (
        "Bị khóa trên bản Web — không nhập mật khẩu thật",
        "Locked in the Web version — do not enter a real password",
    ),
    (
        "Bản Web chưa có đăng nhập và kho bí mật (MB-110/MB-113). Dùng bản Desktop để kiểm tra kết nối thật.",
        "The Web version has no sign-in or secret store yet (MB-110/MB-113). Use the Desktop version to test a real connection.",
    ),
    ("Máy chủ nhận thư (IMAP) *", "Incoming mail server (IMAP) *"),
    ("Máy chủ", "Server"),
    ("Cổng", "Port"),
    ("SSL/TLS bắt buộc", "SSL/TLS required"),
    ("Máy chủ gửi thư (SMTP) *", "Outgoing mail server (SMTP) *"),
    (
        "SSL/TLS 465 hoặc STARTTLS 587",
        "SSL/TLS 465 or STARTTLS 587",
    ),
    ("Chỉ kết nối mã hóa. ", "Encrypted connections only. "),
    (
        "Chứng chỉ máy chủ luôn được kiểm tra; không có tùy chọn bỏ qua lỗi chứng chỉ.",
        "The server certificate is always checked; there is no option to ignore certificate errors.",
    ),
    ("Kiểm tra kết nối (demo)", "Test connection (demo)"),
    (
        "Đang kết nối, kiểm tra chứng chỉ và đăng nhập (IMAP rồi SMTP; có máy chủ mất hơn 10 giây)…",
        "Connecting, checking the certificate and signing in (IMAP then SMTP; some servers take more than 10 seconds)…",
    ),
    (
        "Chưa đọc được số thư trong INBOX.",
        "Could not read the message count of INBOX.",
    ),
    (
        "IMAP: chứng chỉ hợp lệ, đăng nhập thành công. ",
        "IMAP: certificate valid, sign-in succeeded. ",
    ),
    (
        " Máy chủ hỗ trợ IDLE (nhận thư tức thời).",
        " The server supports IDLE (instant mail delivery).",
    ),
    (
        "Chỉ đọc số lượng thư (EXAMINE, không thay đổi gì trên máy chủ).",
        "Only the message count is read (EXAMINE, nothing is changed on the server).",
    ),
    (
        "SMTP: chứng chỉ hợp lệ, đăng nhập thành công. ",
        "SMTP: certificate valid, sign-in succeeded. ",
    ),
    ("Chưa gửi thư nào.", "No message has been sent."),
    ("SMTP (gửi thư): ", "SMTP (sending): "),
    ("Chưa lưu được tài khoản: ", "Could not save the account: "),
    (
        "Khi lưu: mật khẩu vào Keychain của hệ điều hành, thông tin tài khoản và tiêu đề thư vào cơ sở dữ liệu trên máy này. MailBox chỉ đọc hộp thư, không thay đổi gì trên máy chủ.",
        "On save: the password goes to the operating system Keychain, account details and message subjects to the database on this computer. MailBox only reads the mailbox and changes nothing on the server.",
    ),
    ("Quay lại", "Back"),
    ("Đang lưu và đồng bộ…", "Saving and syncing…"),
    ("Lưu tài khoản và đồng bộ", "Save account and sync"),
    (
        "Kiểm tra kết nối — kịch bản minh họa",
        "Connection test — sample scenarios",
    ),
    (
        "Bản demo không mở kết nối mạng. Chọn một kết quả để xem giao diện cho từng trường hợp.",
        "The demo opens no network connection. Pick a result to see the interface for each case.",
    ),
    ("Kịch bản kết quả", "Result scenarios"),
    ("Tiếp tục", "Continue"),
    ("Đã lưu tài khoản", "Account saved"),
    (
        "Nội dung thư và các thư mục khác chưa được tải trong bản này (MB-130).",
        "Message content and the other folders are not downloaded in this version (MB-130).",
    ),
    (
        "Đã xem hết luồng kết nối (demo)",
        "Finished the connection flow (demo)",
    ),
    (
        "Không có tài khoản, mật khẩu hay cấu hình nào được lưu. Bản Web chưa kiểm tra kết nối thật.",
        "No account, password or settings were saved. The Web version cannot test a real connection yet.",
    ),
    ("Thêm tài khoản khác", "Add another account"),
    ("Tùy chỉnh IMAP / SMTP", "Custom IMAP / SMTP"),
    (
        "Nhập địa chỉ email hợp lệ, ví dụ ten@congty.vn.",
        "Enter a valid email address, for example name@company.com.",
    ),
    (
        "Nhập tên máy chủ IMAP, ví dụ imap.congty.vn.",
        "Enter the IMAP server name, for example imap.company.com.",
    ),
    (
        "Cổng phải là số từ 1–65535.",
        "The port must be a number from 1 to 65535.",
    ),
    (
        "Nhập tên máy chủ SMTP, ví dụ smtp.congty.vn.",
        "Enter the SMTP server name, for example smtp.company.com.",
    ),
    ("Đang dò cấu hình", "Detecting settings"),
    ("Cần mật khẩu ứng dụng", "App password needed"),
    ("Lỗi chứng chỉ", "Certificate error"),
    ("Sai thông tin đăng nhập", "Wrong sign-in details"),
    ("IMAP được, SMTP lỗi", "IMAP works, SMTP fails"),
    ("Hoàn tất (demo)", "Done (demo)"),
    (
        "Đang tìm cấu hình phù hợp… (giả lập, không có kết nối mạng)",
        "Looking for a matching configuration… (simulated, no network connection)",
    ),
    (
        "Tài khoản bật xác thực 2 bước: cần tạo mật khẩu ứng dụng tại nhà cung cấp.",
        "The account uses 2-step verification: create an app password with your provider.",
    ),
    (
        "Sai thông tin đăng nhập. Kiểm tra mật khẩu ứng dụng.",
        "Wrong sign-in details. Check the app password.",
    ),
    (
        "Nhận thư được (IMAP) nhưng máy chủ gửi thư từ chối kết nối. Thử STARTTLS cổng 587.",
        "Receiving mail works (IMAP) but the sending server refused the connection. Try STARTTLS on port 587.",
    ),
    (
        "Kịch bản minh họa kết thúc. Không có tài khoản hay mật khẩu nào được lưu.",
        "The sample scenario has ended. No account or password was saved.",
    ),
];
