//! "Trang" theo nghĩa đăng nhập: tên miền gốc (eTLD+1) của host. Cookie đăng nhập thường đặt cho cả miền
//! (`.google.com`), nên tiện ích phải xin quyền cả miền (`*://*.google.com/*`): quyền một host làm Chrome giấu
//! cookie cả miền (đã thử, notes/auth-plan.md §4.2). Tiện ích không có API danh sách hậu tố công khai nên app
//! tính và đưa mẫu quyền cho popup.

use url::{Host, Url};

/// Tên miền gốc của `host` (đã viết thường): theo danh sách hậu tố công khai, gồm phần private
/// (`user.github.io`). IP và host một nhãn (máy nội bộ, `localhost`) giữ nguyên.
pub fn of_host(host: &str) -> String {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.starts_with('[') || host.parse::<std::net::Ipv4Addr>().is_ok() {
        return host;
    }
    psl::domain_str(&host).map_or(host.clone(), str::to_string)
}

/// Trang của một link http(s); `None` nếu không phải link web.
pub fn of_url(raw: &str) -> Option<String> {
    let url = Url::parse(raw).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = match url.host()? {
        Host::Domain(domain) => domain.to_string(),
        Host::Ipv4(address) => address.to_string(),
        Host::Ipv6(address) => format!("[{address}]"),
    };
    Some(of_host(&host))
}

/// Mẫu quyền host cho tiện ích: `*://*.google.com/*` (gồm cả `google.com`); IP và host nội bộ thì đúng host.
pub fn permission_pattern(site: &str) -> String {
    if site.starts_with('[') || site.parse::<std::net::Ipv4Addr>().is_ok() || !site.contains('.') {
        format!("*://{site}/*")
    } else {
        format!("*://*.{site}/*")
    }
}

/// Miền của một cookie (`.google.com`, `drive.google.com`) có thuộc trang `site` không.
pub fn owns(site: &str, cookie_domain: &str) -> bool {
    let domain = cookie_domain.trim_start_matches('.').to_ascii_lowercase();
    domain == site || domain.ends_with(&format!(".{site}"))
}

/// Trang nhạy cảm (cổng chính phủ, quân đội, ngân hàng, ví/thanh toán): không tự kèm cookie dù đã cho phép,
/// mỗi lượt phải bấm xác nhận trong popup (câu Q10 của kế hoạch, học từ Motrix).
pub fn is_sensitive(site: &str) -> bool {
    const LABELS: &[&str] = &["gov", "mil", "gouv", "gob", "govt"];
    const SITES: &[&str] = &[
        "paypal.com",
        "venmo.com",
        "wise.com",
        "revolut.com",
        "stripe.com",
        "chase.com",
        "wellsfargo.com",
        "citi.com",
        "hsbc.com",
        "americanexpress.com",
        "coinbase.com",
        "binance.com",
        "kraken.com",
        "momo.vn",
        "zalopay.vn",
        "vnpay.vn",
        "payoo.vn",
        "acb.com.vn",
        "bidv.com.vn",
        "tpb.vn",
        "vib.com.vn",
        "shb.com.vn",
        "ocb.com.vn",
    ];
    let labels: Vec<&str> = site.split('.').collect();
    SITES.contains(&site)
        || labels.iter().any(|label| LABELS.contains(label))
        || labels.first().is_some_and(|first| first.contains("bank"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_registrable_domain() {
        assert_eq!(of_host("drive.google.com"), "google.com");
        assert_eq!(of_host("DRIVE.usercontent.Google.com."), "google.com");
        assert_eq!(of_host("www.bbc.co.uk"), "bbc.co.uk");
        assert_eq!(of_host("alice.github.io"), "alice.github.io");
        assert_eq!(of_host("www.youtube.com"), "youtube.com");
        assert_eq!(of_host("dichvucong.gov.vn"), "dichvucong.gov.vn");
        assert_eq!(of_host("127.0.0.1"), "127.0.0.1");
        assert_eq!(of_host("localhost"), "localhost");
        assert_eq!(of_host("intranet"), "intranet");
        assert_eq!(of_url("https://[::1]:8080/a").as_deref(), Some("[::1]"));
        assert_eq!(
            of_url("https://m.facebook.com/watch?v=1").as_deref(),
            Some("facebook.com")
        );
        assert_eq!(of_url("file:///c:/a.txt"), None);
    }

    #[test]
    fn builds_permission_patterns() {
        assert_eq!(permission_pattern("google.com"), "*://*.google.com/*");
        assert_eq!(permission_pattern("127.0.0.1"), "*://127.0.0.1/*");
        assert_eq!(permission_pattern("intranet"), "*://intranet/*");
    }

    #[test]
    fn cookies_belong_to_their_site_only() {
        assert!(owns("google.com", ".google.com"));
        assert!(owns("google.com", "drive.google.com"));
        assert!(owns("google.com", "Google.com"));
        assert!(!owns("google.com", "evilgoogle.com"));
        assert!(!owns("google.com", "google.com.evil.example"));
        assert!(!owns("google.com", "youtube.com"));
    }

    #[test]
    fn flags_government_banks_and_payments() {
        for site in [
            "irs.gov",
            "dichvucong.gov.vn",
            "service.gov.uk",
            "vietcombank.com.vn",
            "paypal.com",
            "army.mil",
        ] {
            assert!(is_sensitive(site), "{site}");
        }
        for site in [
            "google.com",
            "youtube.com",
            "dropbox.com",
            "facebook.com",
            "governance.org",
        ] {
            assert!(!is_sensitive(site), "{site}");
        }
    }
}
