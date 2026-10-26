use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::entries::{Source, parse_message};
use super::*;

const NOW: i64 = 1_790_000_000;
const PAGE: &str = "https://www.instagram.com/nasa/";

/// Thư mục tạm của journal; xoá khi bài thử xong (journal đóng trước vì khai báo sau).
struct Scratch(std::path::PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn journal() -> (Scratch, Journal) {
    let root = std::env::temp_dir().join(format!("utd-social-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let journal = Journal::open(root.join("utd.db")).unwrap();
    (Scratch(root), journal)
}

fn message(part: u32, extra: Value) -> Message {
    let mut value = json!({ "listingId": "sb-test0001", "part": part });
    for (key, field) in extra.as_object().unwrap() {
        value[key] = field.clone();
    }
    parse_message(PAGE, &value, NOW).unwrap()
}

fn photo(id: &str, source: &str) -> Value {
    json!({ "id": id, "source": source, "kind": "photo", "url": format!("https://scontent.cdninstagram.com/{id}.jpg") })
}

/// Bộ gom sự kiện như hộp đang mở.
fn collector() -> (Sink, Arc<Mutex<Vec<SocialEvent>>>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let kept = events.clone();
    (Arc::new(move |event| {
        kept.lock().unwrap().push(event);
        true
    }), events)
}

fn kinds(events: &Arc<Mutex<Vec<SocialEvent>>>) -> Vec<String> {
    events
        .lock()
        .unwrap()
        .iter()
        .map(|event| match event {
            SocialEvent::Head { .. } => "head".into(),
            SocialEvent::Profile { .. } => "profile".into(),
            SocialEvent::Entries { entries } => format!("entries:{}", entries.len()),
            SocialEvent::Done { total, complete, error } => format!("done:{total}:{complete}:{}", error.is_some()),
        })
        .collect()
}

#[test]
fn first_part_opens_the_dialog_later_parts_stream_into_it() {
    let (_directory, journal) = journal();
    let mut registry = Registry::default();
    let now = Instant::now();
    let head = json!({ "head": { "id": "528817151", "name": "NASA", "handle": "nasa" }, "entries": [photo("1", "posts")] });
    let first = registry.receive(message(0, head), &journal, now);
    assert_eq!(first.opened.as_ref().map(|request| request.listing_id.as_str()), Some("sb-test0001"));
    assert!(first.reply.go);

    // Hộp mở sau đợt đầu: nhận lại phần đã có.
    let (sink, events) = collector();
    registry.watch("sb-test0001", sink).unwrap();
    assert_eq!(kinds(&events), ["head", "profile", "entries:1"]);

    // Mục trùng (reel cũng nằm trong bài viết) chỉ giữ lần đầu.
    let second = registry.receive(message(1, json!({ "entries": [photo("1", "reels"), photo("2", "reels")] })), &journal, now);
    assert!(second.opened.is_none());
    assert!(second.reply.go);
    let done = registry.receive(message(2, json!({ "done": { "complete": true } })), &journal, now);
    assert!(!done.reply.go, "xong thì tiện ích dừng");
    assert!(done.finished && !first.finished && !second.finished, "chỉ đợt cuối đưa UTD lên trước");
    assert_eq!(kinds(&events), ["head", "profile", "entries:1", "entries:1", "done:2:true:false"]);
    let (key, entries) = registry.entries("sb-test0001").unwrap();
    assert_eq!(key.as_deref(), Some("instagram.com:528817151"));
    assert_eq!(entries.iter().map(|entry| entry.source).collect::<Vec<_>>(), [Source::Posts, Source::Reels]);
}

#[test]
fn stop_and_source_choices_reach_the_extension_on_its_next_part() {
    let (_directory, journal) = journal();
    let mut registry = Registry::default();
    let now = Instant::now();
    registry.receive(message(0, json!({})), &journal, now);
    let (sink, events) = collector();
    registry.watch("sb-test0001", sink).unwrap();
    assert!(registry.control("sb-test0001", Control { go: true, sources: Some(vec![Source::Reels]), since: Some(1_700_000_000) }));
    let reply = registry.receive(message(1, json!({ "entries": [photo("1", "posts")] })), &journal, now).reply;
    assert_eq!(serde_json::to_value(&reply).unwrap(), json!({ "go": true, "sources": ["reels"], "since": 1_700_000_000 }));

    // Dừng: hộp thấy "đã dừng" ngay; đợt sau của tiện ích được bảo dừng, mục tới muộn vẫn giữ.
    registry.control("sb-test0001", Control::STOP);
    assert_eq!(kinds(&events).last().unwrap(), "done:1:false:false");
    let late = registry.receive(message(2, json!({ "entries": [photo("2", "posts")] })), &journal, now);
    assert!(!late.reply.go);
    assert!(!registry.control("sb-unknown01", Control::STOP));
}

#[test]
fn closed_or_expired_listings_tell_the_extension_to_stop() {
    let (_directory, journal) = journal();
    let mut registry = Registry::default();
    let now = Instant::now();
    registry.receive(message(0, json!({})), &journal, now);
    registry.listings.remove("sb-test0001");
    let orphan = registry.receive(message(1, json!({ "entries": [photo("1", "posts")] })), &journal, now);
    assert!(!orphan.reply.go && orphan.opened.is_none(), "đợt sau của danh sách đã đóng không mở lại hộp");

    registry.receive(message(0, json!({})), &journal, now);
    let later = now + IDLE_EXPIRY + Duration::from_secs(1);
    assert!(!registry.receive(message(1, json!({})), &journal, later).reply.go, "hết hạn");
    assert!(registry.watch("sb-test0001", collector().0).is_err());
}

#[test]
fn a_dialog_that_went_away_stops_the_listing() {
    let (_directory, journal) = journal();
    let mut registry = Registry::default();
    let now = Instant::now();
    registry.receive(message(0, json!({})), &journal, now);
    registry.watch("sb-test0001", Arc::new(|_| false)).unwrap();
    let reply = registry.receive(message(1, json!({ "entries": [photo("1", "posts")] })), &journal, now).reply;
    assert!(!reply.go);
}

#[test]
fn downloaded_items_are_marked_from_the_profile_memory() {
    let (_directory, journal) = journal();
    let pick = crate::media::channel::ChannelPick { key: "instagram.com:528817151".into(), video_id: "2".into() };
    crate::media::channel::remember(&journal, &pick, "task-2").unwrap();
    let mut registry = Registry::default();
    let now = Instant::now();
    let head = json!({ "head": { "id": "528817151", "name": "NASA" }, "entries": [photo("1", "posts"), photo("2", "posts")] });
    registry.receive(message(0, head), &journal, now);
    let (_, entries) = registry.entries("sb-test0001").unwrap();
    assert_eq!(entries.iter().map(|entry| entry.downloaded).collect::<Vec<_>>(), [false, true]);
}

#[test]
fn too_many_listings_drop_the_oldest_and_the_entry_cap_ends_the_listing() {
    let (_directory, journal) = journal();
    let mut registry = Registry::default();
    let start = Instant::now();
    for index in 0..=MAX_LISTINGS {
        let value = json!({ "listingId": format!("sb-many{index:04}"), "part": 0 });
        let message = parse_message(PAGE, &value, NOW).unwrap();
        registry.receive(message, &journal, start + Duration::from_secs(index as u64));
    }
    assert_eq!(registry.listings.len(), MAX_LISTINGS);
    assert!(!registry.listings.contains_key("sb-many0000"));

    let mut registry = Registry::default();
    registry.receive(message(0, json!({})), &journal, start);
    let mut part = 1;
    let mut reply = Control { go: true, sources: None, since: None };
    while reply.go && part < 100 {
        let batch: Vec<Value> = (0..entries::MAX_PART_ENTRIES).map(|index| photo(&format!("{part}-{index}"), "posts")).collect();
        reply = registry.receive(message(part, json!({ "entries": batch })), &journal, start).reply;
        part += 1;
    }
    assert!(!reply.go);
    let (_, entries) = registry.entries("sb-test0001").unwrap();
    assert_eq!(entries.len(), MAX_ENTRIES);
    let done = registry.listings["sb-test0001"].finish.clone().unwrap();
    assert!(done.error.unwrap().starts_with("[social.limited]"));
}
