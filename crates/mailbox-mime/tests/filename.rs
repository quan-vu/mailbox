use mailbox_mime::safe_filename;

#[test]
fn names_are_reduced_to_one_harmless_component() {
    for (input, expected) in [
        ("report.pdf", "report.pdf"),
        ("../../x", "x"),
        ("..\\..\\windows\\system32\\evil.dll", "evil.dll"),
        ("/etc/passwd", "passwd"),
        ("C:\\Users\\a\\b.txt", "b.txt"),
        ("....//....//secret", "secret"),
        (".bashrc", "bashrc"),
        ("..", "attachment"),
        ("", "attachment"),
        ("   ", "attachment"),
        ("a\0b\r\nc.txt", "abc.txt"),
        ("invoice\u{202e}fdp.exe", "invoicefdp.exe"),
        ("name. ", "name"),
        ("CON", "_CON"),
        ("nul.txt", "_nul.txt"),
        ("a:b*c?d\"e<f>g|h.txt", "a_b_c_d_e_f_g_h.txt"),
        ("Báo cáo quý 3.pdf", "Báo cáo quý 3.pdf"),
        ("~/.ssh/id_rsa", "id_rsa"),
    ] {
        assert_eq!(safe_filename(input), expected, "{input:?}");
    }
    let long = format!("{}.pdf", "a".repeat(500));
    let out = safe_filename(&long);
    assert!(out.chars().count() <= 120 && out.ends_with(".pdf"));
    let out = safe_filename(&"é".repeat(500));
    assert!(out.chars().count() <= 120);
}
