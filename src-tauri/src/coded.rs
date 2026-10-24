//! Lỗi trả về từ lệnh (`Result<_, String>`) có mã ổn định cho giao diện dịch: `"[nhóm.tên] câu tiếng Việt"`.
//! Giao diện tiếng Việt hiện nguyên câu (giữ chi tiết như tên công cụ, số MB); ngôn ngữ khác dịch theo
//! `err.<nhóm.tên>` trong src/shared/i18n/locales (`commandErrorText`). scripts/i18n-check.mjs kiểm mọi mã có
//! câu dịch. Câu không mã vẫn hiện nguyên, nên lỗi mới chưa kịp gắn mã không hỏng gì.

/// Bỏ mã ở đầu câu, khi ghép câu này vào giữa câu khác.
pub(crate) fn plain(message: &str) -> &str {
    match message.strip_prefix('[').and_then(|rest| rest.split_once("] ")) {
        Some((code, text)) if is_code(code) => text,
        _ => message,
    }
}

fn is_code(code: &str) -> bool {
    code.contains('.')
        && code
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_'))
}

#[cfg(test)]
mod tests {
    use super::plain;

    #[test]
    fn strips_only_a_leading_code() {
        assert_eq!(plain("[tool.timeout] Nguồn tải phản hồi quá lâu."), "Nguồn tải phản hồi quá lâu.");
        assert_eq!(plain("Không có mã."), "Không có mã.");
        assert_eq!(plain("[Ghi chú] không phải mã"), "[Ghi chú] không phải mã");
        assert_eq!(plain("Lỗi [tool.timeout] giữa câu"), "Lỗi [tool.timeout] giữa câu");
    }
}
