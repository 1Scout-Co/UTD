//! Bài thử hàng đợi: phần chạy giả (ghi lại task được gọi chạy) và một bài với engine thật + server
//! cục bộ chậm (127.0.0.1) để thấy chỉ `limit` download chạy cùng lúc.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use utd_engine::{DownloadControl, DownloadEngine, DownloadRequest, EngineOptions};

use super::*;

struct Fixture {
    root: PathBuf,
    journal: Arc<Journal>,
    /// Giờ tạo tăng dần: nhiều task tạo trong cùng một mili giây vẫn có thứ tự rõ.
    clock: AtomicUsize,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("utd-queue-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let journal = Arc::new(Journal::open(root.join("utd.db")).unwrap());
        Self {
            root,
            journal,
            clock: AtomicUsize::new(1),
        }
    }

    fn task(&self, name: &str) -> String {
        let dir = self.root.to_string_lossy().into_owned();
        let mut task =
            DownloadTask::new(format!("https://example.com/{name}"), name, dir, 16).unwrap();
        task.created_at = 1_000_000 + self.clock.fetch_add(1, Ordering::SeqCst) as i64;
        self.journal.save_task(&task).unwrap();
        task.id
    }

    fn complete(&self, id: &str) {
        let mut task = self.journal.load_task(id).unwrap().unwrap();
        task.status = TaskStatus::Completed;
        task.hash_state = utd_engine::HashState::Unavailable;
        task.total_bytes = Some(0);
        task.completed_at = Some(task.created_at);
        self.journal.save_task(&task).unwrap();
    }

    fn tasks(&self, count: usize) -> Vec<String> {
        (0..count)
            .map(|index| self.task(&format!("f{index}.bin")))
            .collect()
    }

    fn status(&self, id: &str) -> TaskStatus {
        self.journal.load_task(id).unwrap().unwrap().status
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Ghi lại task được gọi chạy; bài thử tự gọi `release` thay cho lượt tải kết thúc.
#[derive(Default)]
struct Fake {
    launched: Mutex<Vec<String>>,
    changed: Mutex<Vec<(String, TaskStatus)>>,
    busy: Mutex<HashSet<String>>,
    /// Task chưa sẵn sàng (như task media chờ công cụ tự tải).
    held: Mutex<HashSet<String>>,
    retries: AtomicUsize,
}

impl Fake {
    fn launched(&self) -> Vec<String> {
        self.launched.lock().unwrap().clone()
    }
}

impl Runtime for Fake {
    fn launch(&self, task: &DownloadTask, _entry: &Entry) -> Launch {
        if self.busy.lock().unwrap().remove(&task.id) {
            return Launch::Busy;
        }
        self.launched.lock().unwrap().push(task.id.clone());
        Launch::Started
    }

    fn changed(&self, task: &DownloadTask) {
        self.changed
            .lock()
            .unwrap()
            .push((task.id.clone(), task.status));
    }

    fn retry_later(&self) {
        self.retries.fetch_add(1, Ordering::SeqCst);
    }

    fn ready(&self, task: &DownloadTask) -> bool {
        !self.held.lock().unwrap().contains(&task.id)
    }
}

fn entries(ids: &[String]) -> Vec<Entry> {
    ids.iter().cloned().map(Entry::new).collect()
}

fn started(fx: &Fixture) -> (Arc<DownloadQueue>, Arc<Fake>) {
    let queue = DownloadQueue::new(fx.journal.clone());
    let fake = Arc::new(Fake::default());
    queue.start(fake.clone());
    (queue, fake)
}

#[test]
fn runs_three_at_a_time_in_order() {
    let fx = Fixture::new();
    let (queue, fake) = started(&fx);
    assert_eq!(queue.limit(), DEFAULT_LIMIT);
    let ids = fx.tasks(5);
    let queued = queue.submit(entries(&ids), Place::Back).unwrap();
    assert_eq!(queued.len(), 5);
    assert!(queued.iter().all(|task| task.status == TaskStatus::Queued));
    assert_eq!(fake.launched(), ids[..3]);
    assert_eq!(queue.waiting(), ids[3..]);
    assert_eq!(fx.status(&ids[4]), TaskStatus::Queued);

    queue.release(&ids[1]);
    assert_eq!(fake.launched(), ids[..4], "xong một cái thì chạy cái kế");
    queue.release(&ids[0]);
    queue.release(&ids[0]);
    assert_eq!(fake.launched(), ids, "nhả hai lần không chạy thừa");
    assert!(queue.waiting().is_empty());
}

/// Task chờ công cụ không giữ chỗ: task sau chạy trước; sẵn sàng rồi thì chạy theo đúng thứ tự hàng.
#[test]
fn tasks_that_are_not_ready_keep_their_place_without_a_slot() {
    let fx = Fixture::new();
    let (queue, fake) = started(&fx);
    let ids = fx.tasks(6);
    fake.held.lock().unwrap().extend([ids[0].clone(), ids[1].clone()]);
    queue.submit(entries(&ids), Place::Back).unwrap();
    assert_eq!(fake.launched(), ids[2..5], "đủ 3 chỗ cho task sẵn sàng");
    assert_eq!(queue.waiting(), [ids[0].clone(), ids[1].clone(), ids[5].clone()]);
    assert_eq!(fx.status(&ids[0]), TaskStatus::Queued);

    fake.held.lock().unwrap().clear();
    queue.pump();
    assert_eq!(fake.launched(), ids[2..5], "còn đủ giới hạn thì không chạy thêm");
    queue.release(&ids[2]);
    assert_eq!(fake.launched().last(), Some(&ids[0]), "tới lượt thì task đầu hàng chạy trước");
    assert!(queue.pause(&ids[1]).unwrap(), "task chờ công cụ vẫn tạm dừng được như task chờ lượt");
    queue.release(&ids[3]);
    assert_eq!(fake.launched().last(), Some(&ids[5]));
    assert!(queue.waiting().is_empty());
}

#[test]
fn front_keeps_the_batch_order_and_skips_duplicates() {
    let fx = Fixture::new();
    let (queue, fake) = started(&fx);
    let ids = fx.tasks(7);
    queue.submit(entries(&ids[..5]), Place::Back).unwrap();
    queue
        .submit(
            entries(&[ids[5].clone(), ids[6].clone(), ids[3].clone()]),
            Place::Front,
        )
        .unwrap();
    assert_eq!(
        queue.waiting(),
        vec![
            ids[5].clone(),
            ids[6].clone(),
            ids[3].clone(),
            ids[4].clone()
        ],
        "lô chen đầu hàng giữ thứ tự, task đã chờ giữ chỗ cũ"
    );
    queue.submit(entries(&ids[..1]), Place::Front).unwrap();
    assert_eq!(
        fake.launched(),
        ids[..3],
        "task đang chạy không vào hàng lần nữa"
    );
}

#[test]
fn pausing_a_waiting_task_takes_it_out_of_line() {
    let fx = Fixture::new();
    let (queue, fake) = started(&fx);
    let ids = fx.tasks(5);
    queue.submit(entries(&ids), Place::Back).unwrap();

    assert!(queue.pause(&ids[3]).unwrap());
    assert_eq!(fx.status(&ids[3]), TaskStatus::Paused);
    assert!(
        fake.changed
            .lock()
            .unwrap()
            .contains(&(ids[3].clone(), TaskStatus::Paused))
    );
    assert!(
        !queue.pause(&ids[0]).unwrap(),
        "đang chạy: người gọi huỷ token"
    );

    queue.release(&ids[0]);
    assert_eq!(fake.launched().last(), Some(&ids[4]), "bỏ qua task đã dừng");

    // Tiếp tục ⇒ đầu hàng.
    queue.submit(entries(&ids[3..4]), Place::Front).unwrap();
    assert_eq!(fx.status(&ids[3]), TaskStatus::Queued);
    queue.release(&ids[1]);
    assert_eq!(fake.launched().last(), Some(&ids[3]));
}

#[test]
fn removed_or_finished_tasks_are_skipped() {
    let fx = Fixture::new();
    let (queue, fake) = started(&fx);
    queue.set_limit(1).unwrap();
    let ids = fx.tasks(4);
    queue.submit(entries(&ids), Place::Back).unwrap();
    queue.forget(&ids[1]);
    fx.journal.delete_task(&ids[1]).unwrap();
    fx.journal.delete_task(&ids[2]).unwrap();
    queue.release(&ids[0]);
    assert_eq!(fake.launched(), vec![ids[0].clone(), ids[3].clone()]);

    let done = fx.task("done.bin");
    fx.complete(&done);
    assert!(
        queue
            .submit(entries(&[done]), Place::Back)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_busy_task_keeps_its_place_and_is_retried() {
    let fx = Fixture::new();
    let (queue, fake) = started(&fx);
    let ids = fx.tasks(2);
    fake.busy.lock().unwrap().insert(ids[0].clone());
    queue.submit(entries(&ids), Place::Back).unwrap();
    assert_eq!(fake.retries.load(Ordering::SeqCst), 1);
    assert_eq!(fake.launched(), [ids[1].clone()], "task bận không chặn task sau");
    assert_eq!(queue.waiting(), [ids[0].clone()]);
    queue.pump();
    assert_eq!(fake.launched(), [ids[1].clone(), ids[0].clone()]);
}

/// Task bận (đang Di chuyển…) không nhảy lên trước task media đang chờ công cụ, và suốt lúc bận
/// không chặn task sẵn sàng phía sau (trước đây `push_front` rồi dừng hẳn lượt chạy).
#[test]
fn a_busy_task_neither_jumps_ahead_nor_blocks_ready_tasks() {
    let fx = Fixture::new();
    let (queue, fake) = started(&fx);
    queue.set_limit(1).unwrap();
    let ids = fx.tasks(4);
    let (running, media, moving, next) = (&ids[0], &ids[1], &ids[2], &ids[3]);
    fake.held.lock().unwrap().insert(media.clone());
    queue.submit(entries(&ids), Place::Back).unwrap();
    assert_eq!(fake.launched(), [running.clone()]);
    fake.busy.lock().unwrap().insert(moving.clone());
    queue.release(running);
    assert_eq!(fake.launched(), [running.clone(), next.clone()], "còn chỗ ⇒ task sau chạy");
    assert_eq!(queue.waiting(), [media.clone(), moving.clone()], "task bận ở đúng chỗ cũ");
    fake.held.lock().unwrap().clear();
    queue.release(next);
    assert_eq!(fake.launched().last(), Some(media), "task media vẫn đứng trước task bận");
}

#[test]
fn limit_changes_apply_without_stopping_running_tasks() {
    let fx = Fixture::new();
    let (queue, fake) = started(&fx);
    let ids = fx.tasks(6);
    queue.submit(entries(&ids), Place::Back).unwrap();
    assert_eq!(queue.set_limit(1).unwrap(), 1);
    queue.release(&ids[0]);
    queue.release(&ids[1]);
    assert_eq!(
        fake.launched().len(),
        3,
        "còn một task chạy ⇒ chưa chạy thêm"
    );
    queue.release(&ids[2]);
    assert_eq!(fake.launched().len(), 4);
    assert_eq!(queue.set_limit(99).unwrap(), MAX_LIMIT);
    assert_eq!(fake.launched(), ids);
    assert_eq!(
        DownloadQueue::new(fx.journal.clone()).limit(),
        MAX_LIMIT,
        "giới hạn được lưu"
    );
    assert_eq!(queue.set_limit(0).unwrap(), 1);
}

#[test]
fn nothing_starts_while_frozen_for_an_update() {
    let fx = Fixture::new();
    let (queue, fake) = started(&fx);
    queue.set_limit(1).unwrap();
    let ids = fx.tasks(2);
    queue.submit(entries(&ids), Place::Back).unwrap();
    queue.freeze();
    queue.release(&ids[0]);
    assert_eq!(fake.launched().len(), 1);
    queue.thaw();
    assert_eq!(fake.launched(), ids);
}

#[test]
fn reopening_the_app_continues_in_the_same_order() {
    let fx = Fixture::new();
    let ids = fx.tasks(6);
    {
        let (queue, fake) = started(&fx);
        queue.set_limit(2).unwrap();
        queue.submit(entries(&ids[..5]), Place::Back).unwrap();
        // Task mới thêm sau, chen đầu: thứ tự chờ không còn theo giờ tạo.
        queue.submit(entries(&ids[5..]), Place::Front).unwrap();
        queue.pause(&ids[3]).unwrap();
        assert_eq!(fake.launched(), ids[..2]);
        // Engine đã đưa hai task đang chạy sang `downloading` rồi app tắt ngang.
        fx.journal
            .set_task_status(&ids[0], TaskStatus::Downloading)
            .unwrap();
        fx.journal
            .set_task_status(&ids[1], TaskStatus::Verifying)
            .unwrap();
    }

    let front = interrupted(&fx.journal);
    assert_eq!(front, ids[..2]);
    fx.journal.mark_interrupted().unwrap();
    let queue = DownloadQueue::new(fx.journal.clone());
    queue.restore(&front).unwrap();
    assert_eq!(
        queue.waiting(),
        vec![
            ids[0].clone(),
            ids[1].clone(),
            ids[5].clone(),
            ids[2].clone(),
            ids[4].clone()
        ]
    );
    assert_eq!(fx.status(&ids[0]), TaskStatus::Queued);
    assert_eq!(
        fx.status(&ids[3]),
        TaskStatus::Paused,
        "task đã dừng vẫn dừng"
    );
    let fake = Arc::new(Fake::default());
    queue.start(fake.clone());
    assert_eq!(queue.limit(), 2);
    assert_eq!(fake.launched(), ids[..2]);
}

#[test]
fn restore_without_a_saved_order_uses_creation_time() {
    let fx = Fixture::new();
    let ids = fx.tasks(3);
    let queue = DownloadQueue::new(fx.journal.clone());
    queue.restore(&[]).unwrap();
    assert_eq!(
        queue.waiting(),
        ids,
        "journal cũ: task `queued` chưa từng chạy"
    );
}

// --- Engine thật: server cục bộ trả file chậm, đếm số download chạy cùng lúc. ---

const BODY: usize = 48 * 1024;

fn serve_slowly() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || answer(stream));
        }
    });
    port
}

/// Không hỗ trợ Range: engine tải một luồng; thân gửi từng khúc để các lượt tải chồng nhau.
fn answer(mut stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 2048];
    while !buffer.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(count) => buffer.extend_from_slice(&chunk[..count]),
        }
    }
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {BODY}\r\nConnection: close\r\n\r\n"
    );
    if stream.write_all(head.as_bytes()).is_err() || buffer.starts_with(b"HEAD") {
        return;
    }
    for _ in 0..6 {
        std::thread::sleep(Duration::from_millis(60));
        if stream.write_all(&[7u8; BODY / 6]).is_err() {
            return;
        }
    }
}

#[derive(Default)]
struct Counters {
    running: AtomicUsize,
    peak: AtomicUsize,
    order: Mutex<Vec<String>>,
}

struct EngineRuntime {
    engine: Arc<DownloadEngine>,
    queue: Weak<DownloadQueue>,
    counters: Arc<Counters>,
}

impl Runtime for EngineRuntime {
    fn launch(&self, task: &DownloadTask, entry: &Entry) -> Launch {
        let counters = self.counters.clone();
        let now = counters.running.fetch_add(1, Ordering::SeqCst) + 1;
        counters.peak.fetch_max(now, Ordering::SeqCst);
        counters.order.lock().unwrap().push(task.id.clone());
        let (engine, queue) = (self.engine.clone(), self.queue.clone());
        let (id, rename) = (task.id.clone(), entry.rename);
        tokio::spawn(async move {
            let _ = engine.run_new(&id, DownloadControl::new(), rename).await;
            counters.running.fetch_sub(1, Ordering::SeqCst);
            if let Some(queue) = queue.upgrade() {
                queue.release(&id);
            }
        });
        Launch::Started
    }

    fn changed(&self, _task: &DownloadTask) {}

    fn retry_later(&self) {}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_downloads_respect_the_limit_and_order() {
    let fx = Fixture::new();
    let port = serve_slowly();
    let engine =
        Arc::new(DownloadEngine::new(fx.journal.clone(), EngineOptions::default()).unwrap());
    let queue = DownloadQueue::new(fx.journal.clone());
    let counters = Arc::new(Counters::default());
    queue.start(Arc::new(EngineRuntime {
        engine: engine.clone(),
        queue: Arc::downgrade(&queue),
        counters: counters.clone(),
    }));

    let dir = fx.root.join("out").to_string_lossy().into_owned();
    std::fs::create_dir_all(&dir).unwrap();
    let ids: Vec<String> = (0..7)
        .map(|index| {
            let url = format!("http://127.0.0.1:{port}/file-{index}.bin");
            engine
                .enqueue(DownloadRequest::new(url, dir.clone()))
                .unwrap()
                .id
        })
        .collect();
    let batch = ids
        .iter()
        .map(|id| Entry {
            id: id.clone(),
            rename: true,
        })
        .collect();
    queue.submit(batch, Place::Back).unwrap();

    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while ids.iter().any(|id| fx.status(id) != TaskStatus::Completed) {
        assert!(std::time::Instant::now() < deadline, "download không xong");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(counters.peak.load(Ordering::SeqCst), DEFAULT_LIMIT);
    assert_eq!(
        *counters.order.lock().unwrap(),
        ids,
        "chạy đúng thứ tự xếp hàng"
    );
    for index in 0..7 {
        let file = fx.root.join("out").join(format!("file-{index}.bin"));
        assert_eq!(std::fs::metadata(file).unwrap().len(), BODY as u64);
    }
}
