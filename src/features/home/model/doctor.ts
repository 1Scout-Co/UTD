import type { DownloadItem } from "./types"

export type DoctorCategory =
  | "network"
  | "access"
  | "security"
  | "tool"
  | "disk"
  | "unknown"

export interface DoctorReport {
  title: string
  summary: string
  steps: string[]
  retryable: boolean
  category: DoctorCategory
}

export function diagnose(item: DownloadItem): DoctorReport {
  const code = item.errorCode ?? ""
  const message = item.note ?? ""

  if (code === "guard.dangerous_type" || code === "guard.extension_mismatch") {
    return {
      title: "File bị Download Guard cách ly",
      summary:
        "File có loại nguy hiểm hoặc nội dung không khớp với tên. UTD không đưa file ra thư mục tải.",
      steps: [
        "Mở %LOCALAPPDATA%\\UTD-Data\\quarantine để xem file bị giữ.",
        "Chỉ khôi phục nếu bạn biết rõ nguồn file và tin tưởng nó.",
        "Tải lại từ trang chính thức nếu file là bản cài hoặc script.",
      ],
      retryable: false,
      category: "security",
    }
  }
  if (code.startsWith("verify.") || item.hashState === "mismatch") {
    return {
      title: "Hash không khớp",
      summary:
        message ||
        "Nội dung tải về khác với SHA-256 mong đợi. File có thể bị lỗi hoặc đã bị thay đổi.",
      steps: [
        "Kiểm tra lại SHA-256 lấy từ nguồn chính thức.",
        "Xoá mục này rồi tải lại từ đầu, không resume.",
        "Nếu hash vẫn lệch, đừng chạy file; liên hệ nơi phát hành.",
      ],
      retryable: true,
      category: "security",
    }
  }
  if (code.startsWith("network.") || code === "http.408" || code === "http.429") {
    return {
      title: code === "http.429" ? "Máy chủ đang giới hạn" : "Mạng bị gián đoạn",
      summary:
        message ||
        "Kết nối tới máy chủ không ổn định. UTD đã thử lại nhưng chưa xong.",
      steps: [
        "Kiểm tra mạng, VPN hoặc proxy rồi bấm Tiếp tục.",
        "Nếu máy chủ báo 429, chờ vài phút rồi bấm Tiếp tục; UTD tự giảm số kết nối.",
      ],
      retryable: true,
      category: "network",
    }
  }
  if (code === "http.401" || code === "http.403" || code === "media.login") {
    return {
      title: "Cần quyền truy cập",
      summary:
        message ||
        "Máy chủ từ chối hoặc yêu cầu đăng nhập. Bản này chưa gửi cookie/header đăng nhập.",
      steps: [
        "Mở URL trong trình duyệt để kiểm tra quyền truy cập.",
        "Dùng link tải công khai hoặc link có token còn hiệu lực.",
        "Không tải nội dung riêng tư/trả phí khi chưa có quyền.",
      ],
      retryable: false,
      category: "access",
    }
  }
  if (code === "http.404" || code === "http.410" || code === "media.unavailable") {
    return {
      title: "Nguồn không còn tồn tại",
      summary: message || "Máy chủ không tìm thấy file hoặc video đã bị gỡ.",
      steps: [
        "Kiểm tra lại URL trong trình duyệt.",
        "Tìm bản phát hành mới hoặc mirror chính thức.",
      ],
      retryable: false,
      category: "access",
    }
  }
  if (code.startsWith("media.drm")) {
    return {
      title: "Nội dung có DRM",
      summary:
        message || "UTD không vượt qua bảo vệ bản quyền. Đây là giới hạn có chủ đích.",
      steps: [
        "Dùng tính năng tải xuống chính thức của nền tảng nếu được phép.",
        "Không dùng công cụ bẻ DRM.",
      ],
      retryable: false,
      category: "security",
    }
  }
  if (code === "tool.ytdlp_missing" || code === "tool.ffmpeg_missing") {
    return {
      title: "Thiếu công cụ media",
      summary:
        message ||
        "UTD cần yt-dlp/ffmpeg để tải hoặc ghép media. Bản có sẵn trên máy được dùng luôn.",
      steps: [
        "Mở Cài đặt → Công cụ media, bấm “Tải yt-dlp” hoặc “Tải ffmpeg”.",
        "UTD tải bản chính chủ từ GitHub và kiểm SHA-256 trước khi dùng.",
        "Cài xong, bấm Tiếp tục.",
      ],
      retryable: true,
      category: "tool",
    }
  }
  if (code.startsWith("disk.")) {
    return {
      title: "Lỗi ghi đĩa",
      summary:
        message || "Không ghi được file. Ổ đĩa có thể đầy hoặc bị khoá bởi phần mềm khác.",
      steps: [
        "Kiểm tra dung lượng ổ đĩa và quyền ghi thư mục.",
        "Đóng phần mềm diệt virus đang quét file tạm.",
        "Bấm Tiếp tục sau khi giải phóng dung lượng.",
      ],
      retryable: true,
      category: "disk",
    }
  }
  if (code === "file.exists") {
    return {
      title: "File đích đã tồn tại",
      summary: message || "UTD không ghi đè file đang có.",
      steps: [
        "Đổi tên file đích hoặc chọn thư mục khác.",
        "Xoá file cũ nếu chắc chắn không cần.",
      ],
      retryable: true,
      category: "access",
    }
  }
  if (code === "media.unsupported") {
    return {
      title: "URL chưa được hỗ trợ",
      summary: message || "yt-dlp không tìm thấy luồng media phù hợp tại URL này.",
      steps: [
        "Kiểm tra URL có phải trang video công khai.",
        "Trang vừa đổi thì yt-dlp cũ có thể không đọc được: Cài đặt → Công cụ media → Cập nhật.",
        "Thử tải file trực tiếp nếu có link MP4/HLS.",
      ],
      retryable: false,
      category: "tool",
    }
  }
  return {
    title: item.status === "quarantined" ? "File đang bị cách ly" : "Chưa xác định nguyên nhân",
    summary:
      message ||
      "UTD chưa có chẩn đoán riêng cho lỗi này. Thử tiếp tục hoặc tải lại từ đầu.",
    steps: [
      "Bấm Tiếp tục để thử lại; UTD giữ tiến độ đã ghi trong journal.",
      "Nếu lỗi lặp lại, kiểm tra URL, mạng và dung lượng ổ đĩa.",
      "Không chạy file nếu bạn không rõ nguồn gốc.",
    ],
    retryable: item.status !== "quarantined",
    category: "unknown",
  }
}
