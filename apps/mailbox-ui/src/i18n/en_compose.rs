//! English catalog part: compose. Key = the Vietnamese text used in the code.
pub(super) static ENTRIES: &[(&str, &str)] = &[
    ("Gửi", "Send"),
    ("Gửi (demo)", "Send (demo)"),
    ("Văn bản thuần", "Plain text"),
    ("Văn bản thuần trong bản demo", "Plain text in the demo"),
    ("Soạn thư mới", "New message"),
    ("Đóng soạn thư", "Close compose"),
    ("Bỏ bản nháp này?", "Discard this draft?"),
    (
        "Bản nháp chỉ lưu trong phiên demo. Nếu bỏ, nội dung sẽ mất.",
        "The draft is only kept in this demo session. If you discard it, its content is lost.",
    ),
    ("Bỏ bản nháp", "Discard draft"),
    (
        "Bản Web chưa lưu bản nháp và chưa gửi được thư",
        "The Web version does not save drafts or send mail yet",
    ),
    (
        "Bản nháp cục bộ (demo) — mất khi tải lại",
        "Local draft (demo) — lost on reload",
    ),
    ("Đang gửi thư…", "Sending…"),
    (
        "Giữ ứng dụng mở cho tới khi có kết quả. Nội dung thư đang được khóa.",
        "Keep the app open until there is a result. The message is locked.",
    ),
    ("Đã gửi thư", "Message sent"),
    (
        "Máy chủ gửi thư đã nhận thư này. ",
        "The sending server accepted this message. ",
    ),
    ("Chưa gửi được thư", "Could not send the message"),
    (
        "Địa chỉ bị máy chủ từ chối",
        "Address refused by the server",
    ),
    ("Nhập mật khẩu gửi thư", "Enter the sending password"),
    ("Sửa thư", "Edit message"),
    ("Thư có thể đã được gửi", "The message may have been sent"),
    (
        "Kết nối bị ngắt sau khi thư được chuyển cho máy chủ, nên MailBox không biết thư đã đi hay chưa. MailBox sẽ không tự gửi lại. Hãy kiểm tra thư mục Đã gửi hoặc hỏi người nhận trước khi quyết định.",
        "The connection dropped after the message was handed to the server, so MailBox does not know whether it went out. MailBox will not resend it by itself. Check the Sent folder or ask the recipient before you decide.",
    ),
    (
        "Chưa thấy thư này trong thư mục Đã gửi trên máy chủ. Điều đó không chứng minh được là thư chưa gửi: nhiều máy chủ không tự lưu thư đã gửi.",
        "This message is not in the Sent folder on the server yet. That does not prove it was not sent: many servers do not save sent mail by themselves.",
    ),
    (
        "Chưa kiểm tra được thư mục Đã gửi. Kiểm tra mạng rồi thử lại.",
        "Could not check the Sent folder. Check the network and try again.",
    ),
    ("Kiểm tra thư mục Đã gửi", "Check the Sent folder"),
    ("Thư đã tới — đánh dấu đã gửi", "It arrived — mark as sent"),
    ("Vẫn gửi lại…", "Send again anyway…"),
    (
        "Nếu gửi lại, người nhận có thể nhận hai thư.",
        "If you send again, the recipient may get two messages.",
    ),
    ("Từ:", "From:"),
    (
        "Danh tính gửi luôn hiển thị.",
        "The sending identity is always shown.",
    ),
    (
        "Thư gốc được nhận qua tài khoản ",
        "The original message was received via account ",
    ),
    (
        ". Bạn đang trả lời bằng danh tính khác — người nhận sẽ thấy địa chỉ gửi khác.",
        ". You are replying from a different identity — recipients will see a different sender address.",
    ),
    ("Đến:", "To:"),
    (
        "Người nhận Bcc được ẩn và không bị lộ khi Trả lời tất cả.",
        "Bcc recipients are hidden and are not exposed on Reply all.",
    ),
    (
        "Có địa chỉ email không hợp lệ (đánh dấu đỏ). Sửa hoặc xóa trước khi gửi.",
        "An email address is not valid (marked red). Fix or remove it before sending.",
    ),
    ("Chủ đề:", "Subject:"),
    ("Tiêu đề email", "Email subject"),
    ("Định dạng (chưa hỗ trợ)", "Formatting (not supported yet)"),
    ("Làm lại", "Redo"),
    ("Danh sách", "List"),
    ("Chèn liên kết", "Insert link"),
    ("Nội dung email", "Email body"),
    ("Viết email của bạn tại đây…", "Write your email here…"),
    ("Đính kèm tệp từ máy", "Attach files from this computer"),
    ("Chọn tệp đính kèm", "Choose attachments"),
    (
        "Thêm tệp minh họa (không tải lên)",
        "Add sample files (not uploaded)",
    ),
    (
        "Không có trên máy này — gỡ hoặc đính kèm lại",
        "Not on this computer — remove it or attach it again",
    ),
    ("Chữ ký: Công ty (mặc định)", "Signature: Company (default)"),
    ("Mẫu thư và liên hệ", "Templates and contacts"),
    ("Mẫu thư của bạn", "Your templates"),
    (
        "Điền {{name}} và {{email}} từ người nhận đầu tiên. Áp dụng mẫu sẽ hỏi trước khi ghi đè nội dung.",
        "Fills {{name}} and {{email}} from the first recipient. Applying a template asks before overwriting the content.",
    ),
    (
        "Chưa có mẫu nào. Tạo mẫu trong Cài đặt › Chữ ký.",
        "No templates yet. Create one in Settings › Signature.",
    ),
    ("Tạo mẫu", "Create template"),
    ("Ghi đè nội dung?", "Overwrite the content?"),
    ("Áp dụng mẫu", "Apply template"),
    ("Mẫu thư đề xuất", "Suggested templates"),
    (
        "Áp dụng mẫu sẽ hỏi trước khi ghi đè nội dung.",
        "Applying a template asks before overwriting the content.",
    ),
    ("Gợi ý người liên hệ", "Suggested contacts"),
    ("Từ dữ liệu minh họa.", "From sample data."),
    ("Bản này chưa gửi được thư", "This version cannot send mail"),
    (
        "Gửi thư thật hiện chỉ có trên bản Desktop. Thư này ",
        "Sending real mail is only available in the Desktop version. This message ",
    ),
    ("sẽ không được gửi", "will not be sent"),
    (" tới ai.", " to anyone."),
    ("Bản demo không gửi thư", "The demo does not send mail"),
    (
        "Chế độ demo không kết nối SMTP. Thư này ",
        "Demo mode has no SMTP connection. This message ",
    ),
    (
        " tới ai. Bản nháp vẫn ở đây để bạn tiếp tục chỉnh.",
        " to anyone. The draft stays here so you can keep editing.",
    ),
    ("Đã hiểu", "Got it"),
    (
        "Bản nháp sẽ bị xóa khỏi máy này và không khôi phục được. Thư trên máy chủ không bị ảnh hưởng.",
        "The draft will be removed from this computer and cannot be restored. Mail on the server is not affected.",
    ),
    ("Giữ bản nháp", "Keep draft"),
    (
        "Chưa xóa được bản nháp. Thử lại.",
        "Could not delete the draft. Try again.",
    ),
    (
        "Gửi lại dù thư có thể đã đi?",
        "Send again although the message may have gone out?",
    ),
    (
        "Lần gửi trước có thể đã thành công. Nếu gửi lại, người nhận có thể nhận hai thư giống nhau. Chỉ gửi lại khi bạn đã kiểm tra thư mục Đã gửi hoặc hỏi người nhận.",
        "The previous send may have succeeded. If you send again, the recipient may get two identical messages. Only send again after you checked the Sent folder or asked the recipient.",
    ),
    ("Không gửi lại", "Do not resend"),
    ("Tôi hiểu, gửi lại", "I understand, send again"),
    ("Mật khẩu gửi thư (SMTP)", "Sending password (SMTP)"),
    (
        "Máy chủ gửi thư không chấp nhận mật khẩu của tài khoản. Nếu nhà cung cấp dùng mật khẩu riêng cho việc gửi thư (hoặc mật khẩu ứng dụng), nhập vào đây. MailBox kiểm tra với máy chủ qua kết nối mã hóa rồi lưu trong Keychain của hệ điều hành — không lưu trong cơ sở dữ liệu hay tệp cài đặt.",
        "The sending server did not accept the account password. If your provider uses a separate password for sending (or an app password), enter it here. MailBox checks it with the server over an encrypted connection and stores it in the operating system Keychain — not in the database or settings files.",
    ),
    ("Mật khẩu SMTP", "SMTP password"),
    ("Lưu và gửi lại", "Save and send again"),
    ("Địa chỉ không hợp lệ", "Invalid address"),
    ("Nhập email, nhấn Enter", "Enter an email, press Enter"),
    ("Bản nháp và thư đang gửi", "Drafts and outgoing mail"),
    (
        "Bản nháp chỉ lưu trên máy này; chưa đồng bộ lên thư mục Nháp của máy chủ.",
        "Drafts are kept on this computer only; they are not synced to the server's Drafts folder.",
    ),
    ("Tải lại", "Reload"),
    (
        "Không đọc được bản nháp trên máy này. Thử tải lại.",
        "Could not read the drafts on this computer. Try reloading.",
    ),
    ("Chưa có bản nháp nào", "No drafts yet"),
    (
        "Thư bạn đang viết sẽ tự lưu ở đây và vẫn còn sau khi mở lại ứng dụng.",
        "Mail you are writing is saved here automatically and stays after you reopen the app.",
    ),
    ("(Chưa có người nhận)", "(No recipients yet)"),
    ("Mở", "Open"),
    (
        "Xóa mục này khỏi máy?",
        "Remove this item from this computer?",
    ),
    (
        "Bản lưu trên máy này sẽ bị xóa và không khôi phục được. Thư đã gửi và thư trên máy chủ không bị ảnh hưởng.",
        "The copy on this computer will be deleted and cannot be restored. Sent mail and mail on the server are not affected.",
    ),
    ("Giữ lại", "Keep"),
    (
        "Chưa xóa được. Thư đang gửi không thể xóa.",
        "Could not delete it. A message that is being sent cannot be deleted.",
    ),
    (
        "Thêm ít nhất một người nhận.",
        "Add at least one recipient.",
    ),
    (
        "Có địa chỉ email không hợp lệ.",
        "An email address is not valid.",
    ),
    ("Chủ đề đang trống.", "The subject is empty."),
    (
        "Chủ đề có ký tự xuống dòng hoặc ký tự điều khiển.",
        "The subject contains line breaks or control characters.",
    ),
    (
        "Tệp đính kèm vượt giới hạn 25 MB.",
        "Attachments exceed the 25 MB limit.",
    ),
    (
        "Có tệp đính kèm chưa nằm trên máy này. Gỡ tệp đó hoặc đính kèm lại từ máy.",
        "An attachment is no longer on this computer. Remove it or attach it again from this computer.",
    ),
    ("Chọn tài khoản gửi.", "Choose a sending account."),
    (
        "Bản nháp sẽ được lưu trên máy này khi bạn bắt đầu viết",
        "The draft will be saved on this computer once you start writing",
    ),
    ("Có thay đổi chưa lưu", "There are unsaved changes"),
    ("Đang lưu bản nháp…", "Saving draft…"),
    (
        "Đã lưu bản nháp trên máy này",
        "Draft saved on this computer",
    ),
    (
        "Chưa lưu được bản nháp — nội dung vẫn ở đây, đừng đóng ứng dụng",
        "Could not save the draft — the content is still here, do not close the app",
    ),
    (
        "Máy chủ gửi thư từ chối thư này. Thư chưa được gửi. Kiểm tra nội dung hoặc thử lại sau.",
        "The sending server rejected this message. It was not sent. Check the content or try again later.",
    ),
    (
        "Máy chủ từ chối một số địa chỉ nhận nên thư chưa được gửi cho ai. Sửa hoặc gỡ các địa chỉ bị từ chối rồi gửi lại.",
        "The server refused some recipient addresses, so the message was sent to no one. Fix or remove the refused addresses and send again.",
    ),
    (
        "Máy chủ gửi thư không chấp nhận mật khẩu đã lưu. Thư chưa được gửi. Nhập mật khẩu gửi thư (SMTP) để thử lại.",
        "The sending server did not accept the saved password. The message was not sent. Enter the sending (SMTP) password to try again.",
    ),
    (
        "Không kết nối được tới máy chủ gửi thư. Thư chưa được gửi. Kiểm tra mạng rồi thử lại.",
        "Could not connect to the sending server. The message was not sent. Check the network and try again.",
    ),
    (
        "Chứng chỉ của máy chủ gửi thư không hợp lệ nên MailBox dừng lại. Thư chưa được gửi. MailBox không cho phép bỏ qua kiểm tra này.",
        "The sending server's certificate is not valid, so MailBox stopped. The message was not sent. MailBox does not allow bypassing this check.",
    ),
    (
        "Thư lớn hơn giới hạn của máy chủ gửi thư. Thư chưa được gửi. Gỡ bớt tệp đính kèm rồi gửi lại.",
        "The message is larger than the sending server's limit. It was not sent. Remove some attachments and send again.",
    ),
    (
        "Máy chủ gửi thư không hỗ trợ địa chỉ có ký tự ngoài bảng mã ASCII. Thư chưa được gửi. Dùng địa chỉ không dấu.",
        "The sending server does not support addresses with characters outside ASCII. The message was not sent. Use an address without accents.",
    ),
    (
        "Thư có địa chỉ hoặc tiêu đề không hợp lệ. Thư chưa được gửi. Sửa thư rồi gửi lại.",
        "The message has an invalid address or subject. It was not sent. Fix the message and send again.",
    ),
    (
        "Không đọc được một tệp đính kèm (tệp đã bị xóa, đổi tên hoặc chuyển chỗ). Thư chưa được gửi. Sửa thư và đính kèm lại.",
        "Could not read an attachment (the file was deleted, renamed or moved). The message was not sent. Fix the message and attach it again.",
    ),
    (
        "Tệp đính kèm vượt giới hạn 25 MB. Thư chưa được gửi. Gỡ bớt tệp rồi gửi lại.",
        "Attachments exceed the 25 MB limit. The message was not sent. Remove some files and send again.",
    ),
    (
        "Địa chỉ gửi không khớp với tài khoản đăng nhập. Thư chưa được gửi. Chọn lại tài khoản gửi.",
        "The sending address does not match the signed-in account. The message was not sent. Choose the sending account again.",
    ),
    (
        "Ứng dụng đã đóng trước khi thư được chuyển cho máy chủ. Thư chưa được gửi. Bấm Thử lại để gửi.",
        "The app closed before the message was handed to the server. It was not sent. Press Retry to send.",
    ),
    (
        "Không ghi được dữ liệu trên máy này (cơ sở dữ liệu cục bộ hoặc Keychain). Thư chưa được gửi. Thử lại.",
        "Could not write data on this computer (local database or Keychain). The message was not sent. Try again.",
    ),
    (
        "Đang lưu bản sao vào thư mục Đã gửi trên máy chủ…",
        "Saving a copy in the Sent folder on the server…",
    ),
    (
        "Đã lưu một bản sao vào thư mục Đã gửi trên máy chủ.",
        "A copy was saved in the Sent folder on the server.",
    ),
    (
        "Máy chủ đã tự lưu thư vào thư mục Đã gửi; MailBox không tạo bản thứ hai.",
        "The server already saved the message in the Sent folder; MailBox did not create a second copy.",
    ),
    (
        "Thư đã được gửi, nhưng chưa lưu được bản sao vào thư mục Đã gửi trên máy chủ.",
        "The message was sent, but a copy could not be saved in the Sent folder on the server.",
    ),
    ("Đang gửi", "Sending"),
    ("Chưa gửi được", "Not sent"),
    ("Có thể đã gửi", "May have been sent"),
    ("Kế hoạch ra mắt sản phẩm", "Product launch plan"),
    (
        "Thông báo kế hoạch, timeline và phân công",
        "Announce the plan, timeline and assignments",
    ),
    (
        "Chào team,\n\nMình gửi kế hoạch ra mắt gồm timeline, phạm vi và phân công.\n\n• Thời gian ra mắt:\n• Phạm vi:\n• Việc tiếp theo:\n\nCảm ơn cả team!",
        "Hi team,\n\nHere is the launch plan with the timeline, scope and assignments.\n\n• Launch date:\n• Scope:\n• Next steps:\n\nThanks, everyone!",
    ),
    ("Thông báo nội bộ", "Internal announcement"),
    (
        "Cập nhật thông tin quan trọng cho team",
        "Share important updates with the team",
    ),
    (
        "Chào mọi người,\n\nMình xin thông báo:\n\nTrân trọng.",
        "Hi everyone,\n\nI would like to announce:\n\nBest regards.",
    ),
    ("Mời họp dự án", "Project meeting invitation"),
    (
        "Lời mời họp và nội dung thảo luận",
        "Meeting invitation and discussion topics",
    ),
    (
        "Chào mọi người,\n\nMời mọi người tham gia buổi họp:\n• Thời gian:\n• Nội dung:\n\nCảm ơn!",
        "Hi everyone,\n\nYou are invited to the meeting:\n• Time:\n• Topics:\n\nThanks!",
    ),
    ("Báo cáo tiến độ", "Progress report"),
    (
        "Tổng hợp kết quả và việc tiếp theo",
        "Summarize results and next steps",
    ),
    (
        "Chào team,\n\nKết quả tuần này:\n\nViệc tiếp theo:\n\nTrân trọng.",
        "Hi team,\n\nThis week's results:\n\nNext steps:\n\nBest regards.",
    ),
    ("Cảm ơn & phản hồi", "Thanks & feedback"),
    (
        "Gửi lời cảm ơn và ghi nhận đóng góp",
        "Say thanks and acknowledge contributions",
    ),
    ("Cảm ơn", "Thank you"),
    (
        "Chào bạn,\n\nCảm ơn bạn đã hỗ trợ.\n\nTrân trọng.",
        "Hi,\n\nThank you for your help.\n\nBest regards.",
    ),
];
