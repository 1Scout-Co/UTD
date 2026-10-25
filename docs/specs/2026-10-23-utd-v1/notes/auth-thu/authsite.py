"""THỬ (prototype, không phải code UTD): trang cần đăng nhập đơn giản chạy ở 127.0.0.1.

- GET /dologin          -> đặt cookie phiên `sid` (HttpOnly) rồi chuyển về /; /login chỉ là trang HTML
- GET /file.mp4         -> 200 + dữ liệu nếu có cookie `sid=ok`, không thì 302 về /login (như trang thật)
- GET /domlogin         -> cookie `dom` cho cả miền utd.test (Domain=) + `hostonly`
- GET /hop.mp4          -> 302 sang host khác (localhost:<port>/file.mp4) để xem cookie có bị mang theo không
- GET /page             -> trang HTML có <video src=/file.mp4> (cho extractor generic)
Mỗi request in ra: method, path, Host, có Cookie hay không (chỉ tên cookie, KHÔNG in giá trị).
"""
import http.server
import socketserver
import sys

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8765
BODY = bytes(range(256)) * 4096  # 1 MiB


def cookie_names(header):
    if not header:
        return "-"
    return ",".join(part.split("=", 1)[0].strip() for part in header.split(";"))


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def trace(self):
        print(
            f"{self.command} {self.path} host={self.headers.get('Host')} cookies={cookie_names(self.headers.get('Cookie'))} "
            f"auth={'yes' if self.headers.get('Authorization') else 'no'}",
            flush=True,
        )

    def send_plain(self, code, body=b"", headers=()):
        self.send_response(code)
        for key, value in headers:
            self.send_header(key, value)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)

    def do_HEAD(self):
        self.do_GET()

    def do_GET(self):
        self.trace()
        cookie = self.headers.get("Cookie") or ""
        logged_in = "sid=ok" in cookie
        if self.path.startswith("/domlogin"):
            # Như Google: cookie phiên đặt cho cả miền (Domain=utd.test) + một cookie riêng host.
            self.send_plain(302, headers=[("Set-Cookie", "dom=1; Domain=utd.test; Path=/; HttpOnly"), ("Set-Cookie", "hostonly=1; Path=/"), ("Location", "/")])
        elif self.path.startswith("/dologin"):
            self.send_plain(302, headers=[("Set-Cookie", "sid=ok; Path=/; HttpOnly"), ("Location", "/")])
        elif self.path.startswith("/login"):
            self.send_plain(200, b"<html><title>Dang nhap</title><form>login</form></html>", [("Content-Type", "text/html")])
        elif self.path.startswith("/file.mp4"):
            if not logged_in:
                self.send_plain(302, headers=[("Location", "/login")])
                return
            rng = self.headers.get("Range")
            if rng and rng.startswith("bytes="):
                start, _, end = rng[6:].partition("-")
                start = int(start)
                end = int(end) if end else len(BODY) - 1
                part = BODY[start : end + 1]
                self.send_plain(
                    206,
                    part,
                    [
                        ("Content-Type", "video/mp4"),
                        ("Accept-Ranges", "bytes"),
                        ("Content-Range", f"bytes {start}-{end}/{len(BODY)}"),
                        ("Set-Cookie", "rot=2; Path=/"),
                    ],
                )
            else:
                self.send_plain(200, BODY, [("Content-Type", "video/mp4"), ("Accept-Ranges", "bytes"), ("Set-Cookie", "rot=1; Path=/")])
        elif self.path.startswith("/slow.bin"):
            # File 24 MiB gửi chậm (để tắt app giữa chừng), cũng chỉ trả khi có sid=ok.
            if not logged_in:
                self.send_plain(302, headers=[("Location", "/login")])
                return
            total = 24 * 1024 * 1024
            start, end = 0, total - 1
            rng = self.headers.get("Range")
            status = 200
            if rng and rng.startswith("bytes="):
                first, _, last = rng[6:].partition("-")
                start = int(first)
                end = min(int(last), total - 1) if last else total - 1
                status = 206
            self.send_response(status)
            self.send_header("Content-Type", "application/octet-stream")
            self.send_header("Accept-Ranges", "bytes")
            self.send_header("Content-Length", str(end - start + 1))
            if status == 206:
                self.send_header("Content-Range", f"bytes {start}-{end}/{total}")
            self.end_headers()
            if self.command == "HEAD":
                return
            import time
            position = start
            try:
                while position <= end:
                    size = min(64 * 1024, end - position + 1)
                    self.wfile.write(bytes((position + i) % 251 for i in range(size)))
                    position += size
                    time.sleep(0.05)
            except (ConnectionResetError, BrokenPipeError, ConnectionAbortedError):
                pass
        elif self.path.startswith("/hop.mp4"):
            self.send_plain(302, headers=[("Location", f"http://localhost:{PORT}/file.mp4")])
        elif self.path.startswith("/page"):
            html = b"<html><head><title>Trang thu</title></head><body><video src='/file.mp4'></video></body></html>"
            self.send_plain(200, html, [("Content-Type", "text/html; charset=utf-8")])
        else:
            self.send_plain(200, b"home", [("Content-Type", "text/plain")])


class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True
    allow_reuse_address = True


with Server(("127.0.0.1", PORT), Handler) as server:
    print(f"listening {PORT}", flush=True)
    server.serve_forever()
