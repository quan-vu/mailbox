# Báo cáo cuối phiên 2026-11-15 — MailBox Desktop 1.0

Người nhận: chủ dự án và agent của phiên sau. Đọc file này trước, rồi `AGENTS.md`, `CLAUDE.md`,
`docs/06-delivery/TRACKING.md`. Bản giao ca trước đó đã bỏ khỏi repo.

## 1. Trạng thái hiện tại

| Hạng mục | Trạng thái |
|---|---|
| Phiên bản | 1.0.0 (Desktop). Tag `v1.0.0` đã có trên GitHub, trỏ vào `afc4ebd` |
| Nhánh | `main`, đã push. Sau tag có thêm các commit sửa link, sửa CI và tài liệu |
| macOS | Bản release build trên máy, cài một bản duy nhất tại `/Applications/MailBox.app`, chưa ký. Bản đang cài build từ `740fe50` |
| Windows | **Chưa có bản cài.** Hai lần build trên GitHub Actions đều thất bại (mục 3) |
| Linux | Chưa build, chưa kiểm |
| Web | Vẫn là bản demo, không có đăng nhập |
| Test trên máy | `cargo test --workspace`: 526 pass, 0 fail, 25 ignored (cả với rustc 1.97 và 1.100) |
| Lint | fmt sạch; clippy workspace, UI desktop, UI web sạch với rustc 1.97; workspace và UI desktop sạch với rustc 1.100 |

Số liệu task trong TRACKING: 27 DONE, 3 IN PROGRESS, 1 MOCK, 6 NOT STARTED, 1 AUTHORED.

## 2. Việc đã làm trong phiên này

- **S-08** (AUDIT): mỗi lệnh IMAP chỉ đọc tối đa 32 MiB phản hồi. Thân thư giữ giới hạn riêng 25 MB.
- **S-09** (AUDIT): link trong thư không tự mở; bấm vào hiện hộp ghi tên miền thật và địa chỉ đầy
  đủ, cảnh báo với `http` và tên miền `xn--`, từ chối địa chỉ đánh lừa.
  - Bản sửa đầu tiên không dùng được: bấm link không ra gì, vì WKWebView không báo điều hướng của
    khung sandbox. Bản sửa thứ hai cho link điều hướng khung chính, app chặn rồi hỏi.
  - Đã bấm tay trên macOS: 5 link mẫu trên bản dev, link https trên bản cài.
- Sửa panic của `guard_links` với chữ nhiều byte (mọi thư HTML tiếng Việt có link).
- Thư demo "Cảnh báo bảo mật cho tài khoản" có 5 link mẫu để thử hộp xác nhận mà không cần tài khoản.
- Tag `v1.0.0`, push `main` và tag.
- Sửa 3 test kho dữ liệu hỏng trên Windows (xoá file SQLite đang mở).
- Sửa 2 cảnh báo mới của rustc 1.100 mà CI coi là lỗi.

## 3. Build Windows trên GitHub Actions

Workflow `.github/workflows/ci.yml` chạy khi push tag `v*` hoặc chạy tay. Nó cài Rust stable mới
nhất (hiện là 1.100), còn máy phát triển dùng 1.97.

| Lần | Run | Commit | Kết quả |
|---|---|---|---|
| 1 | 43675072909 (tag `v1.0.0`) | `afc4ebd` | Fail ở test: `bodies_survive_reopening_an_existing_database` xoá thư mục khi file SQLite còn mở |
| 2 | 43677148430 (chạy tay, `main`) | `910659d` | Test core và test UI pass. Fail ở lint UI: `clippy::needless_bool`, `unreachable_code` |
| 3 | chưa chạy | `31e9ad1` trở đi | Hai lỗi lint đã sửa và đã kiểm trên máy bằng rustc 1.100 |

Các bước "Install Dioxus CLI", "Bundle Windows desktop app" và upload artifact **chưa từng chạy**,
nên có thể còn lỗi ở đó. Tag `v1.0.0` không chứa các bản sửa CI.

## 4. Việc tồn đọng

Chặn bản Windows:

1. Chạy lại build Windows trên `main` (cần chủ dự án đồng ý; tốn phút CI).
2. Thử bản Windows trên máy thật khi build xong: cài, mở, kết nối, bấm link, lưu tệp đính kèm.
3. Quyết định ra `v1.0.1` để có một tag build được trên Windows.

Chưa kiểm sau các thay đổi cuối:

4. Web E2E (Playwright, desktop-1440) chưa chạy lại sau lần sửa link thứ hai. Lần chạy gần nhất
   (trước đó): 49 pass, 2 skip, 0 fail.
5. Link trong một thư thật đã đồng bộ chưa được bấm thử: bản cài chưa lưu tài khoản nào.
6. `real_send` (SMTP thật) chưa chạy lại trong phiên này; mã SMTP không đổi.
7. Thông báo macOS cho bản chưa ký: chưa thấy banner thật.
8. Migration PostgreSQL và contract test PostgreSQL: chưa chạy.

Lỗi nhỏ đã thấy, chưa sửa:

9. Tiêu đề cửa sổ vẫn ghi "(demo)" khi đang ở Chế độ thật.

Bảo mật còn mở (`docs/04-security/AUDIT_1.0.md`):

10. S-10 (Medium): Windows không gắn Mark-of-the-Web cho tệp đã lưu.
11. S-11 (Medium): `libsqlite3-sys` 0.30.1 qua sqlx 0.8.6 có RUSTSEC-2026-0459; chờ bản sqlx mới.
12. S-12 đến S-29 (Low) và S-30 đến S-36 (Info): chưa xử lý, đã ghi trong AUDIT.
13. Fuzz và load test: chưa chạy.

Cần chủ dự án cung cấp:

14. Apple Developer ID để ký và notarize (MB-302).
15. OAuth client của Google và Microsoft (MB-203 đang là mock có nhãn).
16. MB-111: chủ tài khoản tự đăng nhập IMAP trong app để kiểm bằng tay.

Chưa làm, ngoài phạm vi Desktop 1.0 (`LIMITATIONS_1.0.md`): đăng nhập Web (MB-110), vault Web
(MB-113), triển khai Web (MB-300), sao lưu (MB-301), CI nhiều nhà cung cấp (MB-303), đồng bộ thư
mục Nháp của máy chủ, tìm kiếm trên máy chủ, đồng bộ khi app đóng, giao diện mobile.

## 5. Câu hỏi cần hỏi lại chủ dự án khi quay lại

1. Có chạy lại build Windows trên `main` không?
2. Giữ `v1.0.0` (không có bản Windows) hay ra `v1.0.1` sau khi CI xanh?
3. Có ghim phiên bản Rust trong CI (ví dụ 1.100.0) để cảnh báo mới không làm đỏ build nữa không?
4. Có thêm tài khoản thật vào bản cài để thử link trong thư thật không? (Mật khẩu do chủ dự án
   tự gõ; agent không nhập thay.)
5. Việc tiếp theo ưu tiên gì: bản Windows, ký macOS, OAuth thật, hay các mục S-10/S-11?
6. Có chạy lại Web E2E trước khi làm tiếp không?

## 6. Cách làm tiếp

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo fmt --all -- --check
cargo test --workspace
cargo +1.100.0 clippy --workspace --exclude mailbox-ui --all-targets -- -D warnings
cargo +1.100.0 clippy -p mailbox-ui --no-default-features --features desktop --all-targets -- -D warnings
```

- Toolchain `1.100.0` đã cài trên máy phát triển để kiểm lint giống CI trước khi tốn phút CI.
- Cài bản macOS: `cd apps/mailbox-ui && dx bundle -p mailbox-ui --desktop --release --package-types macos`,
  rồi thay `/Applications/MailBox.app` (chỉ giữ một bản).
- Chạy lại CI Windows bằng tay: `gh workflow run ci.yml --ref main` (chỉ khi chủ dự án yêu cầu).
- Quy tắc giữ nguyên: Desktop trước; build macOS/Linux trên máy; Windows chỉ trên GitHub Actions
  và chỉ khi được yêu cầu; không push tag, không chạy workflow khi chưa được yêu cầu; commit
  không ghi thông tin về agent; không ghi mật khẩu, token hay địa chỉ cá nhân vào repo.

## 7. Cập nhật cuối ngày: chuẩn bị mở repo công khai

Phần này mới hơn các mục 1, 3, 4, 5 ở trên; chỗ nào khác nhau thì theo phần này.

Đã làm:

- Web E2E (Playwright, desktop-1440) chạy lại sau lần sửa link thứ hai: 49 pass, 2 skip, 0 fail.
  Bản Web khi đó build bằng rustc 1.97, trước khi ghim 1.100.
- Rust ghim ở 1.100.0 trong `rust-toolchain.toml` và trong workflow. Mọi gate trên máy chạy lại
  bằng 1.100.0: fmt sạch, clippy workspace / UI desktop / UI web sạch, 526 test pass.
- Workflow Windows giờ **chỉ chạy tay** và phải nêu tag: `gh workflow run ci.yml -f tag=v1.0.1`.
  Push commit, push tag hay pull request không còn khởi động gì. Workflow chưa chạy lần nào ở
  dạng mới.
- README có 6 ảnh chụp (chế độ demo, bản Web 1440×900) trong `docs/screenshots/`.
- Thêm `SECURITY.md`, `CONTRIBUTING.md`; README ghi giấy phép Apache-2.0.
- Gỡ khỏi cây: `MailBox-Rust-Starter/`, `MailBox-Rust-Starter-v0.1.zip` (8 MB),
  `MANIFEST-SHA256.txt`, bản giao ca 2026-11-14.
- Thay bằng placeholder trong cây: địa chỉ hộp thư test, tên miền, tên máy chủ của nhà cung cấp,
  đường dẫn thư mục cá nhân, nhãn Docker nội bộ.

Rà soát bí mật (toàn bộ lịch sử, mọi nhánh): không có mật khẩu, token, khoá riêng, file `.env`
thật hay file cơ sở dữ liệu; không có địa chỉ Gmail cá nhân; tác giả commit dùng địa chỉ
noreply của GitHub.

Còn lại trước khi đổi repo sang công khai (cần chủ dự án quyết):

1. **Lịch sử git vẫn chứa** địa chỉ hộp thư test, tên miền và tên máy chủ mail thật (trong các
   commit cũ của tài liệu), cùng file zip 8 MB. Không phải bí mật, nhưng công khai thì lộ địa
   chỉ và máy chủ. Hai lựa chọn: (a) chấp nhận; (b) tạo lịch sử mới gọn (một commit hoặc lọc
   lịch sử) rồi force-push và tạo lại tag. (b) không hoàn tác được trên GitHub và làm mất 80+
   commit chi tiết.
2. Tag `v1.0.0` không build được trên Windows. Nên ra `v1.0.1` từ `main` rồi chạy workflow cho
   tag đó.
3. Cài đặt GitHub cần bật bằng tay: private vulnerability reporting (SECURITY.md dựa vào nó),
   secret scanning + push protection, bảo vệ nhánh `main`.
4. Nhật ký của hai lần chạy Actions cũ sẽ thành công khai cùng repo (không chứa bí mật).
5. Chưa có bản phát hành nào trên GitHub Releases; bản macOS chưa ký.
6. Tài liệu trong `docs/06-delivery/` (TRACKING, evidence, báo cáo này) là ghi chép nội bộ, có
   chỗ bằng tiếng Việt và có tên nhánh làm việc. Giữ hay bỏ là tuỳ chủ dự án.

Câu hỏi thêm cho chủ dự án: chọn (a) hay (b) ở mục 1; có ra `v1.0.1` và chạy build Windows không;
có giữ `docs/06-delivery/` trong bản công khai không.

## 8. Chốt cuối ngày: lịch sử gọn và v1.0.1

Theo quyết định của chủ dự án ngày 2026-11-15:

- Lịch sử git được gộp thành **một commit gốc** chứa đúng cây mã của 1.0.1. Các mã commit cũ
  nhắc trong tài liệu (TRACKING, CHANGELOG, các mục trên) không còn trong repo.
- Tag cũ `v0.2.0` và `v1.0.0` đã xoá vì chúng giữ lịch sử cũ. Tag hiện hành: `v1.0.1`.
- Lịch sử đầy đủ được sao lưu ra một file bundle trên máy chủ dự án, nằm ngoài repo
  (`mailbox-history-backup-2026-11-15.bundle`, cạnh thư mục repo). Không đưa file này lên mạng.
- GitHub có thể còn giữ các commit cũ theo mã SHA một thời gian và nhật ký hai lần chạy Actions
  cũ vẫn còn. Muốn xoá hẳn thì xoá các lần chạy đó và nhờ GitHub Support dọn, hoặc tạo repo mới.
- Mục 7, điểm 1 và 2 coi như đã xử lý. Điểm 3 đến 6 xem TRACKING.
