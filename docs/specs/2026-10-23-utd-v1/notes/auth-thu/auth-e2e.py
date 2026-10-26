"""THỬ (GĐ1): luồng đăng nhập trọn vẹn trên app debug thật, dữ liệu riêng (UTD_DATA_DIR), không đụng UTD của
chủ dự án. Tin nhắn native giả lập đúng những gì tiện ích gửi (bản 1 không cookie, bản 2 có `auth`).

Trang mẫu: authsite.py ở cổng 8768 (file chỉ trả khi có cookie sid=ok). In từng bước và kết quả kiểm.
"""
import json
import os
import sqlite3
import struct
import subprocess
import sys
import time

app, data, downloads, site_log = sys.argv[1:5]


def site_requests():
    """Số yêu cầu trang thử đã nhận (dòng `GET …`/`HEAD …` authsite.py in ra)."""
    with open(site_log, encoding="utf-8", errors="replace") as log:
        return sum(1 for line in log if line.startswith(("GET ", "HEAD ")))


BASE = "http://127.0.0.1:8768"
PAGE = BASE + "/page"
CANARY = "UTDCANARYe2e" + str(int(time.time()))
os.makedirs(data, exist_ok=True)
os.makedirs(downloads, exist_ok=True)
env = dict(os.environ, UTD_DATA_DIR=data)
started_at = time.time()


def start():
    proc = subprocess.Popen([app], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    config = os.path.join(data, "ipc.json")
    before = os.path.getmtime(config) if os.path.exists(config) else 0
    for _ in range(300):
        if os.path.exists(config) and os.path.getmtime(config) > before:
            break
        time.sleep(0.1)
    time.sleep(1.5)
    return proc


def native(message):
    body = json.dumps(message).encode("utf-8")
    out = subprocess.run(
        [app], input=struct.pack("<I", len(body)) + body, env=dict(env, UTD_NATIVE_HOST="1"), capture_output=True, timeout=30
    ).stdout
    size = struct.unpack("<I", out[:4])[0]
    return json.loads(out[4 : 4 + size].decode("utf-8"))


def db():
    return sqlite3.connect(f"file:{os.path.join(data, 'utd.db')}?mode=ro", uri=True, timeout=5)


def task(url):
    with db() as connection:
        row = connection.execute(
            "SELECT id, status, error_json, auth_sites_json, final_url, downloaded_bytes FROM tasks WHERE url = ? ORDER BY created_at DESC LIMIT 1",
            (url,),
        ).fetchone()
    if not row:
        return None
    error = json.loads(row[2]) if row[2] else None
    return {"id": row[0], "status": row[1], "error": error and error["code"], "auth": row[3], "final": row[4], "bytes": row[5]}


def wait(url, predicate, seconds=60):
    deadline = time.time() + seconds
    while time.time() < deadline:
        current = task(url)
        if current and predicate(current):
            return current
        time.sleep(0.3)
    return task(url)


def cookie(name, value):
    return {"name": name, "value": value, "domain": "127.0.0.1", "hostOnly": True, "path": "/", "secure": False, "expires": None}


AUTH = {"site": "127.0.0.1", "cookies": [cookie("sid", "ok"), cookie("canary", CANARY)], "userAgent": "Mozilla/5.0 UTD-e2e"}

proc = start()
with sqlite3.connect(os.path.join(data, "utd.db"), timeout=5) as connection:
    connection.execute(
        "INSERT OR REPLACE INTO settings (key, value, updated_at) VALUES ('download_dir', ?, ?)",
        (downloads, int(time.time() * 1000)),
    )
try:
    # 1. Lần đầu: chưa cho phép ⇒ tin bản 1, trang đưa về /login ⇒ task "Cần đăng nhập".
    file_url = BASE + "/file.mp4"
    print("1 gửi không cookie:", native({"version": 1, "type": "download", "url": file_url, "pageUrl": PAGE})["ok"])
    print("  task:", wait(file_url, lambda t: t["status"] == "failed"))

    # 2. Popup mở: hỏi việc chờ.
    pending = native({"version": 2, "type": "auth-pending", "url": PAGE})
    print("2 popup hỏi:", pending.get("auth"))

    # 3. Popup "Dùng đăng nhập": gửi cookie ⇒ task chờ tải tiếp.
    renew = native({"version": 2, "type": "auth-renew", "url": PAGE, "auth": AUTH})
    print("3 cho phép:", renew.get("auth"), renew.get("message"))
    done = wait(file_url, lambda t: t["status"] == "completed")
    print("  task:", done, "file:", os.path.getsize(os.path.join(downloads, "file.mp4")) if os.path.exists(os.path.join(downloads, "file.mp4")) else None)

    # 4. Trang đã cho phép: tin bản 2 kèm cookie, file chậm; tắt ngang app giữa chừng.
    slow_url = BASE + "/slow.bin"
    print("4 gửi kèm cookie:", native({"version": 2, "type": "download", "url": slow_url, "pageUrl": PAGE, "auth": AUTH})["ok"])
    print("  đang tải:", wait(slow_url, lambda t: t["bytes"] > 2 * 1024 * 1024, 30))
    proc.kill()
    proc.wait()
    # Đếm dòng yêu cầu (GET/HEAD) của trang thử, không đếm traceback ConnectionResetError lúc giết app.
    time.sleep(1)
    requests_before = site_requests()
    proc = start()
    after = wait(slow_url, lambda t: t["status"] == "failed", 30)
    time.sleep(1)
    print("  mở lại app:", after, "yêu cầu mới tới trang:", site_requests() - requests_before)

    # 5. Popup "Tiếp tục": tải nốt từ phần đã có.
    print("5 popup hỏi:", native({"version": 2, "type": "auth-pending", "url": PAGE}).get("auth", {}).get("pending"))
    renew = native({"version": 2, "type": "auth-renew", "url": PAGE, "auth": AUTH})
    print("  cho phép:", renew.get("auth"))
    done = wait(slow_url, lambda t: t["status"] == "completed", 120)
    size = os.path.getsize(os.path.join(downloads, "slow.bin")) if os.path.exists(os.path.join(downloads, "slow.bin")) else None
    print("  task:", done, "file:", size)
finally:
    proc.kill()
    proc.wait()

# 6. Không sót: giá trị cookie mồi không có trong file nào của dữ liệu app, thư mục tải, %TEMP% (file mới).
hits = []
roots = [data, downloads, os.environ.get("TEMP", "")]
for root in roots:
    for folder, _, files in os.walk(root):
        for name in files:
            path = os.path.join(folder, name)
            try:
                if root.endswith("Temp") and os.path.getmtime(path) < started_at:
                    continue
                if os.path.getsize(path) > 64 * 1024 * 1024:
                    continue
                if CANARY.encode() in open(path, "rb").read():
                    hits.append(path)
            except OSError:
                pass
print("6 file chứa cookie mồi:", hits)
