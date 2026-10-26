// Giữ Alt khi bấm tải ⇒ lần này để trình duyệt tự tải (như IDM; takeover.js). Chỉ báo "vừa bấm kèm Alt", không đọc
// gì của trang. Không khai báo tên ở phạm vi ngoài: dùng chung phạm vi với content.js.
addEventListener(
  "click",
  (event) => {
    if (!event.altKey) return
    try {
      void (globalThis.browser ?? globalThis.chrome).runtime.sendMessage({ type: "utd.takeover-skip" })?.catch?.(() => undefined)
    } catch {
      // Tiện ích vừa nạp lại: trang cũ không còn nói chuyện được.
    }
  },
  true,
)
