//! Mạng của một lượt tải: đổi mạng (Wi-Fi↔4G, VPN), máy vừa ngủ dậy, hay mất hẳn mạng.
//!
//! - Đổi mạng / ngủ dậy: kết nối cũ chết lặng (IP nguồn đã khác, NAT đã quên). Thay vì chờ hết hạn im
//!   `idle_timeout`, tăng `epoch`; kết nối đang chờ byte thấy `epoch` đổi thì đóng và mở lại ngay.
//! - Mất hẳn mạng (không còn route) một lúc mà không nhận byte nào: kết thúc lượt bằng lỗi
//!   `network.offline`; app chờ có mạng rồi tự chạy lại (không thành trạng thái lỗi).

use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use crate::recovery::RouteProbe;

/// Hỏi bảng định tuyến mỗi chừng này (rẻ: một socket UDP, không gửi gói nào).
const CHECK_EVERY: Duration = Duration::from_secs(1);
/// Giờ máy chạy nhanh hơn đồng hồ đơn điệu chừng này giữa hai nhịp ⇒ máy vừa ngủ / ngủ đông.
const SLEEP_GAP: Duration = Duration::from_secs(5);
/// Không có route liền chừng này mà không nhận byte nào ⇒ coi là mất mạng, dừng lượt.
const OFFLINE_AFTER: Duration = Duration::from_secs(3);

/// Số lần mạng đổi, chia cho mọi kết nối của lượt tải.
pub(crate) struct NetEpoch(AtomicU64);

impl Default for NetEpoch {
    fn default() -> Self {
        Self::new()
    }
}

impl NetEpoch {
    pub const fn new() -> Self {
        Self(AtomicU64::new(0))
    }

    pub fn current(&self) -> u64 {
        self.0.load(Ordering::Acquire)
    }

    fn bump(&self) {
        self.0.fetch_add(1, Ordering::AcqRel);
    }
}

pub(crate) struct NetWatch {
    probe: RouteProbe,
    epoch: Arc<NetEpoch>,
    address: Option<IpAddr>,
    checked: Instant,
    wall: SystemTime,
    offline_since: Option<Instant>,
}

/// Kết quả một nhịp theo dõi.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum NetState {
    Fine,
    /// Không có mạng đủ lâu mà không nhận byte: dừng lượt tải.
    Offline,
}

impl NetWatch {
    pub fn new(probe: RouteProbe) -> Self {
        let address = probe.address();
        Self {
            probe,
            epoch: Arc::new(NetEpoch::default()),
            address,
            checked: Instant::now(),
            wall: SystemTime::now(),
            offline_since: None,
        }
    }

    pub fn epoch(&self) -> Arc<NetEpoch> {
        self.epoch.clone()
    }

    /// Gọi mỗi nhịp của vòng lượt tải; `receiving` = có byte mới từ nhịp trước.
    pub fn tick(&mut self, now: Instant, receiving: bool) -> NetState {
        if receiving {
            self.offline_since = None;
        }
        let since = now.saturating_duration_since(self.checked);
        if since < CHECK_EVERY {
            return self.state(now);
        }
        let wall = SystemTime::now();
        let wall_since = wall.duration_since(self.wall).unwrap_or_default();
        let slept = wall_since > since + SLEEP_GAP;
        self.checked = now;
        self.wall = wall;
        let address = self.probe.address();
        let moved = address.is_some() && address != self.address;
        if address.is_some() {
            self.offline_since = None;
        } else if !receiving {
            self.offline_since.get_or_insert(now);
        }
        if slept || moved {
            log::info!(
                "engine.net change slept={slept} moved={moved} epoch={}",
                self.epoch.current() + 1
            );
            self.epoch.bump();
        }
        self.address = address;
        self.state(now)
    }

    fn state(&self, now: Instant) -> NetState {
        match self.offline_since {
            Some(since) if now.saturating_duration_since(since) >= OFFLINE_AFTER => NetState::Offline,
            _ => NetState::Fine,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;
    use std::sync::Mutex;

    use super::*;

    fn fake(addresses: Arc<Mutex<Option<IpAddr>>>) -> RouteProbe {
        RouteProbe::new(move || *addresses.lock().unwrap())
    }

    #[test]
    fn a_new_address_bumps_the_epoch_and_no_route_means_offline() {
        let wifi = Some(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 5)));
        let mobile = Some(IpAddr::V4(Ipv4Addr::new(10, 20, 0, 7)));
        let current = Arc::new(Mutex::new(wifi));
        let mut watch = NetWatch::new(fake(current.clone()));
        let epoch = watch.epoch();
        let start = Instant::now();
        assert_eq!(watch.tick(start + Duration::from_secs(1), true), NetState::Fine);
        assert_eq!(epoch.current(), 0);

        *current.lock().unwrap() = mobile;
        assert_eq!(watch.tick(start + Duration::from_secs(2), false), NetState::Fine);
        assert_eq!(epoch.current(), 1, "đổi Wi-Fi→4G");

        *current.lock().unwrap() = None;
        assert_eq!(watch.tick(start + Duration::from_secs(3), false), NetState::Fine);
        assert_eq!(watch.tick(start + Duration::from_secs(5), false), NetState::Fine);
        assert_eq!(watch.tick(start + Duration::from_secs(7), false), NetState::Offline);
        assert_eq!(epoch.current(), 1, "mất mạng không phải đổi mạng");

        *current.lock().unwrap() = mobile;
        assert_eq!(watch.tick(start + Duration::from_secs(8), false), NetState::Fine);
        assert_eq!(epoch.current(), 2, "có mạng lại: kết nối cũ đã chết, mở lại ngay");
    }

    #[test]
    fn bytes_still_arriving_is_never_offline() {
        let current = Arc::new(Mutex::new(None));
        let mut watch = NetWatch::new(fake(current));
        let start = Instant::now();
        for second in 1..10 {
            assert_eq!(
                watch.tick(start + Duration::from_secs(second), true),
                NetState::Fine,
                "máy chỉ ra mạng qua proxy, không có route mặc định"
            );
        }
    }
}
