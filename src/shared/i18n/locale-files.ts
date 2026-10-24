// URL của từng file dịch: Vite chép mỗi file thành một tệp riêng trong dist/assets (không nằm trong JS), app chỉ
// tải file đang dùng. Tách khỏi i18n.tsx và nạp động: code gọi t() vẫn import được ngoài Vite (bài thử
// Playwright chạy hàm của model trong Node, nơi không có import.meta.glob).
export const LOCALE_FILES = import.meta.glob<string>("./locales/*.json", {
  eager: true,
  query: "?url",
  import: "default",
})
