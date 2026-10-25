import { StrictMode } from "react"
import { createRoot } from "react-dom/client"

import "./index.css"
import App from "./App.tsx"
import { ThemeProvider } from "@/components/theme-provider.tsx"
import { startI18n } from "@/shared/i18n/i18n"
import { blockNativeContextMenu } from "@/shared/window/native-menu"

blockNativeContextMenu()

// Nạp file dịch của ngôn ngữ đã chọn (hoặc của hệ điều hành) trước khi vẽ: không nháy chữ sai ngôn ngữ.
void startI18n()
  .catch((error: unknown) => console.error("UTD i18n:", error))
  .finally(() =>
    createRoot(document.getElementById("root")!).render(
      <StrictMode>
        {/* Giao diện Soniox chỉ có bảng màu sáng: không theo Dark Mode của máy, tắt phím D. Để "dark" thì
            menu chuột phải/⋯ (vẽ ngoài khung Soniox) thành nền đen trên trang sáng. Khoá lưu mới bỏ giá trị
            "dark" cũ của ai lỡ bấm D. */}
        <ThemeProvider defaultTheme="light" storageKey="utd-theme" hotkey={false}>
          <App />
        </ThemeProvider>
      </StrictMode>
    )
  )
