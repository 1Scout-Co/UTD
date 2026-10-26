"""THỬ (GĐ3, tự bắt link tải): app thật với dữ liệu riêng (UTD_DATA_DIR), không đụng UTD của chủ dự án. Tin nhắn
native giả lập đúng tin `takeover` mà extensions/*/takeover.js gửi khi trình duyệt bắt đầu tải.

Trang mẫu: authsite.py ở cổng 8769 (/file.mp4 chỉ trả khi có cookie sid=ok, không thì về /login).
Chạy: python -I takeover-e2e.py <app.exe> <thư mục dữ liệu> <thư mục tải>
In từng bước; cuối cùng "ĐẠT" hoặc "HỎNG".
"""
import json
import os
import sqlite3
import struct
import subprocess
import sys
import time

app, data, downloads = sys.argv[1:4]
BASE = "http://127.0.0.1:8769"
PAGE = BASE + "/page"
CANARY = "UTDCANARYtakeover" + str(int(time.time()))
os.makedirs(data, exist_ok=True)
os.makedirs(downloads, exist_ok=True)
env = dict(os.environ, UTD_DATA_DIR=data)
here = os.path.dirname(os.path.abspath(__file__))
site = subprocess.Popen([sys.executable, "-I", os.path.join(here, "authsite.py"), "8769"], stdout=subprocess.DEVNULL)
failures = []


def check(label, condition, detail=""):
    print(("  ok   " if condition else "  HỎNG ") + label, detail)
    if not condition:
        failures.append(label)


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
        [app], input=struct.pack("<I", len(body)) + body, env=dict(env, UTD_NATIVE_HOST="1"), capture_output=True, timeout=60
    ).stdout
    size = struct.unpack("<I", out[:4])[0]
    return json.loads(out[4 : 4 + size].decode("utf-8"))


def tasks():
    with sqlite3.connect(f"file:{os.path.join(data, 'utd.db')}?mode=ro", uri=True, timeout=5) as connection:
        return connection.execute("SELECT id, url, file_name, status, auth_sites_json FROM tasks").fetchall()


def cookie(name, value):
    return {"name": name, "value": value, "domain": "127.0.0.1", "hostOnly": True, "path": "/", "secure": False, "expires": None}


AUTH = {"site": "127.0.0.1", "cookies": [cookie("sid", "ok"), cookie("canary", CANARY)], "userAgent": "Mozilla/5.0 UTD-e2e"}


def takeover(url, auth=None, name=None):
    message = {"version": 2, "type": "takeover", "url": url, "pageUrl": PAGE, "suggestedName": name}
    if auth:
        message["auth"] = auth
    return native(message)


started_at = time.time()
proc = start()
with sqlite3.connect(os.path.join(data, "utd.db"), timeout=5) as connection:
    connection.execute(
        "INSERT OR REPLACE INTO settings (key, value, updated_at) VALUES ('download_dir', ?, ?)",
        (downloads, int(time.time() * 1000)),
    )
try:
    print("1 file cần đăng nhập, trang chưa dùng đăng nhập (không cookie): UTD không nhận, trình duyệt tải tiếp")
    reply = takeover(BASE + "/file.mp4")
    check("trả lời không nhận", reply["ok"] is False and reply["message"] == "UTD để trình duyệt tải file này.", reply)
    check("không tạo task", tasks() == [])

    print("2 trang web (HTML) thay vì file: không nhận")
    reply = takeover(BASE + "/login")
    check("không nhận", reply["ok"] is False, reply)

    print("3 kèm cookie của cú bấm, tên trình duyệt gợi ý: UTD nhận, tải xong, đúng tên")
    reply = takeover(BASE + "/file.mp4", AUTH, "phim-rieng.mp4")
    check("nhận", reply["ok"] is True and bool(reply.get("taskId")), reply)
    target = os.path.join(downloads, "phim-rieng.mp4")
    for _ in range(200):
        rows = tasks()
        if rows and rows[0][3] == "completed":
            break
        time.sleep(0.3)
    rows = tasks()
    check("task xong, ghi dấu trang 127.0.0.1", len(rows) == 1 and rows[0][3] == "completed" and "127.0.0.1" in (rows[0][4] or ""), rows)
    check("file đủ 1 MiB", os.path.exists(target) and os.path.getsize(target) == 1024 * 1024)
finally:
    proc.kill()
    proc.wait()

print("4 UTD không chạy: native host báo lỗi, tiện ích cho trình duyệt tải tiếp")
reply = takeover(BASE + "/file.mp4", AUTH)
check("không nhận", reply["ok"] is False, reply)
site.kill()

print("5 không sót cookie mồi trong dữ liệu app, thư mục tải, %TEMP% (file mới)")
hits = []
for root in [data, downloads, os.environ.get("TEMP", "")]:
    for folder, _, files in os.walk(root):
        for name in files:
            path = os.path.join(folder, name)
            try:
                if os.path.getmtime(path) < started_at:
                    continue
                with open(path, "rb") as handle:
                    # Đọc từng khúc (chồng mép) để file lớn trong %TEMP% không ngốn RAM.
                    tail = b""
                    while chunk := handle.read(8 << 20):
                        if CANARY.encode() in tail + chunk:
                            hits.append(path)
                            break
                        tail = chunk[-64:]
            except OSError:
                pass
check("0 file chứa mồi", hits == [], hits)
print("ĐẠT" if not failures else f"HỎNG: {failures}")
sys.exit(1 if failures else 0)
