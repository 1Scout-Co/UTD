"""THỬ: app debug chạy với UTD_DATA_DIR riêng; gửi tin nhắn native liên tiếp qua pipe IPC đã gia cố.

Không tạo download nào: chỉ gửi loại `batch` (app mở hộp Thêm hàng loạt, không tải) và một dòng sai token.
Chỉ dừng đúng tiến trình app do script này mở.
"""
import json
import os
import struct
import subprocess
import sys
import time

app, data = sys.argv[1], sys.argv[2]
os.makedirs(data, exist_ok=True)
env = dict(os.environ, UTD_DATA_DIR=data)
proc = subprocess.Popen([app], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
try:
    config_path = os.path.join(data, "ipc.json")
    for _ in range(300):
        if os.path.exists(config_path):
            break
        time.sleep(0.1)
    config = json.load(open(config_path, encoding="utf-8"))
    time.sleep(1.0)

    def native(message):
        body = json.dumps(message).encode("utf-8")
        host_env = dict(env, UTD_NATIVE_HOST="1")
        out = subprocess.run([app], input=struct.pack("<I", len(body)) + body, env=host_env, capture_output=True, timeout=30).stdout
        size = struct.unpack("<I", out[:4])[0]
        return json.loads(out[4 : 4 + size].decode("utf-8"))

    batch = {"version": 1, "type": "batch", "urls": ["https://example.com/a.zip"], "pageUrl": "https://example.com/"}
    for index in range(4):
        reply = native(batch)
        print(f"native #{index + 1}: ok={reply['ok']} message={reply['message']!r}")

    # Nối thẳng pipe, sai token: app phải từ chối.
    with open(r"\\.\pipe" + "\\" + config["pipe"], "r+b", buffering=0) as pipe:
        pipe.write(json.dumps({"token": "f" * len(config["token"]), "url": "https://example.com/a.zip"}).encode() + b"\n")
        print("sai token:", pipe.readline().decode("utf-8").strip())
    reply = native(batch)
    print(f"native sau đó: ok={reply['ok']}")
finally:
    proc.terminate()
    proc.wait(timeout=10)
