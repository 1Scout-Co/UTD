use std::cell::RefCell;

use super::*;

/// Journal trong thư mục tạm; xoá thư mục sau khi đóng journal.
pub(super) struct Scratch {
    root: std::path::PathBuf,
    journal: Option<Journal>,
}

impl Scratch {
    pub(super) fn new() -> Self {
        let root = std::env::temp_dir().join(format!("utd-channel-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let journal = Journal::open(root.join("utd.db")).unwrap();
        Self {
            root,
            journal: Some(journal),
        }
    }

    pub(super) fn journal(&self) -> &Journal {
        self.journal.as_ref().unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        self.journal.take();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Gom sự kiện như giao diện nhận; `open = false` giả hộp đã đóng.
struct Collected {
    events: RefCell<Vec<ListingEvent>>,
    open: bool,
}

impl Collected {
    fn new(open: bool) -> Self {
        Self {
            events: RefCell::new(Vec::new()),
            open,
        }
    }

    fn entries(&self) -> Vec<ChannelEntry> {
        self.events
            .borrow()
            .iter()
            .filter_map(|event| match event {
                ListingEvent::Entries { entries } => Some(entries.clone()),
                _ => None,
            })
            .flatten()
            .collect()
    }
}

impl Sink for Collected {
    fn deliver(&self, event: ListingEvent) -> bool {
        self.events.borrow_mut().push(event);
        self.open
    }
}

const TIKTOK: &str = include_str!("fixtures/tiktok-flat.jsonl");

#[tokio::test]
async fn streams_entries_marks_downloaded_ones_and_drops_duplicates() {
    let scratch = Scratch::new();
    let pick = ChannelPick {
        key: "tiktok.com:MS4wLjABAAAAsample-secUid_0123".into(),
        video_id: "7700000000000000002".into(),
    };
    remember(scratch.journal(), &pick, "task-removed-from-list").unwrap();
    // Dòng rác, dòng trùng và dòng không phải JSON đều bị bỏ qua.
    let first = TIKTOK.lines().next().unwrap();
    let output = format!("WARNING: not json\n{TIKTOK}{first}\n\n");
    let sink = Collected::new(true);
    let mut listing = Listing::new(&sink, scratch.journal(), false);
    let stop = listing
        .read(output.as_bytes(), &CancellationToken::new())
        .await;
    assert_eq!(stop, Stop::Ended);
    let done = listing.finish(stop, true, "");
    assert_eq!(
        done,
        ListingEvent::Done {
            total: 3,
            complete: true,
            error: None
        }
    );

    let events = sink.events.borrow();
    let ListingEvent::Head { head } = &events[0] else {
        panic!("sự kiện đầu là tên kênh: {:?}", events[0]);
    };
    assert_eq!(head.name, "Sample Creator");
    drop(events);
    let entries = sink.entries();
    let ids: Vec<&str> = entries.iter().map(|entry| entry.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "7700000000000000001",
            "7700000000000000002",
            "7700000000000000003"
        ]
    );
    let downloaded: Vec<bool> = entries.iter().map(|entry| entry.downloaded).collect();
    assert_eq!(downloaded, [false, true, false]);
}

#[tokio::test]
async fn stops_when_the_dialog_is_gone_or_cancelled() {
    let scratch = Scratch::new();
    let closed = Collected::new(false);
    let mut listing = Listing::new(&closed, scratch.journal(), false);
    let stop = listing
        .read(TIKTOK.as_bytes(), &CancellationToken::new())
        .await;
    assert_eq!(stop, Stop::Gone, "hộp đóng ⇒ dừng ngay ở sự kiện đầu");

    let sink = Collected::new(true);
    let cancel = CancellationToken::new();
    cancel.cancel();
    let mut listing = Listing::new(&sink, scratch.journal(), false);
    let stop = listing.read(TIKTOK.as_bytes(), &cancel).await;
    assert_eq!(stop, Stop::Cancelled);
    assert_eq!(
        listing.finish(stop, false, ""),
        ListingEvent::Done {
            total: 0,
            complete: false,
            error: None
        }
    );
}

#[test]
fn failures_keep_what_was_listed_and_explain_why() {
    let scratch = Scratch::new();
    let sink = Collected::new(true);
    let mut listing = Listing::new(&sink, scratch.journal(), false);
    assert!(listing.line(TIKTOK.lines().next().unwrap()));
    let stderr = "ERROR: [tiktok:user] sample: This user's account is likely either private or all of their videos are private. Log into an account that has access";
    let ListingEvent::Done {
        total,
        complete,
        error,
    } = listing.finish(Stop::Ended, false, stderr)
    else {
        panic!("phải là done");
    };
    assert_eq!((total, complete), (1, false));
    assert!(error.unwrap().starts_with("[channel.private_all] Tài khoản riêng tư"));
    assert_eq!(sink.entries().len(), 1, "video đã đọc vẫn gửi đi");
}

#[test]
fn explains_common_listing_errors() {
    let cases = [
        (
            "ERROR: [tiktok:user] a: This account does not have any videos posted",
            "[channel.no_videos] Kênh chưa đăng video nào.",
        ),
        (
            "ERROR: [tiktok:user] a: Unable to extract secondary user ID.",
            "[channel.tiktok_refused] TikTok không trả",
        ),
        (
            "ERROR: [youtube:tab] @a: This channel does not exist.",
            "[channel.not_found] Không tìm thấy kênh.",
        ),
        (
            "ERROR: Unable to download webpage: <urlopen error timed out>",
            "[probe.connect] Không kết nối được",
        ),
    ];
    for (stderr, expected) in cases {
        assert!(listing_error(stderr).starts_with(expected), "{stderr}");
    }
}

#[test]
fn asks_youtube_for_approximate_dates_only() {
    let youtube = listing_args("https://www.youtube.com/@a/videos");
    assert!(youtube.contains(&"youtubetab:approximate_date"));
    assert_eq!(youtube.last(), Some(&"https://www.youtube.com/@a/videos"));
    let tiktok = listing_args("https://www.tiktok.com/@a");
    assert!(!tiktok.contains(&"--extractor-args"));
    assert!(tiktok.contains(&"--lazy-playlist") && tiktok.contains(&"--flat-playlist"));
}

#[test]
fn listing_events_are_tagged_for_the_ui() {
    let done = serde_json::to_value(ListingEvent::Done {
        total: 2,
        complete: true,
        error: None,
    })
    .unwrap();
    assert_eq!(
        done,
        serde_json::json!({ "kind": "done", "total": 2, "complete": true, "error": null })
    );
}

#[test]
fn retries_only_when_tiktok_refused_before_any_video() {
    let done = |total: usize, error: Option<&str>| ListingEvent::Done {
        total,
        complete: false,
        error: error.map(str::to_string),
    };
    assert!(worth_retrying(&done(0, Some(TIKTOK_REFUSED))));
    assert!(
        !worth_retrying(&done(3, Some(TIKTOK_REFUSED))),
        "đã có video ⇒ giữ, không chạy lại"
    );
    assert!(!worth_retrying(&done(0, Some("Kênh chưa đăng video nào."))));
    assert!(!worth_retrying(&done(0, None)), "bị huỷ");
}
