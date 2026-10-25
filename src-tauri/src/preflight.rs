//! Cảnh báo trước khi tải (S-22) cho một link hay cả lô: luật nằm ở Download Guard
//! (`utd_engine::safety::warnings`), ở đây chỉ nhận những gì giao diện đã biết về từng link.

use serde::Deserialize;
use utd_engine::safety::{self, LinkFacts, Warning};

/// Những gì giao diện biết về một link: tên người dùng sửa / server gợi ý, kết quả dò (`inspect_link`).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkInput {
    pub url: String,
    #[serde(default)]
    pub final_url: Option<String>,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub content_type: Option<String>,
}

/// Cảnh báo của từng link, cùng thứ tự với đầu vào (link không có gì đáng ngại ⇒ mảng rỗng).
/// Không mở mạng: lô 10 000 link vẫn trả ngay.
#[tauri::command]
pub fn check_links(links: Vec<LinkInput>) -> Result<Vec<Vec<Warning>>, String> {
    if links.len() > crate::batch::MAX_BATCH_ITEMS {
        return Err("too many links".into());
    }
    Ok(links.iter().map(warnings_of).collect())
}

pub(crate) fn warnings_of(link: &LinkInput) -> Vec<Warning> {
    safety::warnings(&LinkFacts {
        url: &link.url,
        final_url: link.final_url.as_deref(),
        file_name: link.file_name.as_deref().unwrap_or(""),
        content_type: link.content_type.as_deref(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use utd_engine::safety::WarningKind;

    fn input(json: &str) -> Vec<LinkInput> {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn answers_each_link_in_order() {
        let links = input(
            r#"[
                {"url": "https://example.com/a.zip"},
                {"url": "http://example.com/setup.exe", "fileName": "setup.exe"},
                {"url": "https://example.com/dl?id=7", "finalUrl": "https://cdn.example.com/x",
                 "fileName": "hoa-don.pdf", "contentType": "application/x-msdownload"}
            ]"#,
        );
        let found = check_links(links).unwrap();
        assert_eq!(found.len(), 3);
        assert!(found[0].is_empty());
        let kinds: Vec<WarningKind> = found[1].iter().map(|warning| warning.kind).collect();
        assert_eq!(kinds, [WarningKind::DangerousType, WarningKind::InsecureHttp]);
        assert_eq!(found[2][0].kind, WarningKind::MimeMismatch);
    }

    #[test]
    fn caps_the_batch_size() {
        let many: Vec<LinkInput> = (0..=crate::batch::MAX_BATCH_ITEMS)
            .map(|index| LinkInput {
                url: format!("https://example.com/{index}.bin"),
                final_url: None,
                file_name: None,
                content_type: None,
            })
            .collect();
        assert!(check_links(many).is_err());
    }
}
