import { StrictMode } from "react"
import { createRoot } from "react-dom/client"

import "./index.css"
import App from "./App.tsx"
import { ThemeProvider } from "@/components/theme-provider.tsx"
import { startI18n } from "@/shared/i18n/i18n"

// Nạp file dịch của ngôn ngữ đã chọn (hoặc của hệ điều hành) trước khi vẽ: không nháy chữ sai ngôn ngữ.
void startI18n()
  .catch((error: unknown) => console.error("UTD i18n:", error))
  .finally(() =>
    createRoot(document.getElementById("root")!).render(
      <StrictMode>
        <ThemeProvider>
          <App />
        </ThemeProvider>
      </StrictMode>
    )
  )
