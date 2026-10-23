; Móc cho bộ cài NSIS của Tauri (tauri.conf.json > bundle > windows > nsis > installerHooks).
; Viết theo template installer.nsi của @tauri-apps/cli 2.12.4 (tag tauri-cli-v2.12.4). Đổi bản CLI thì đọc lại
; template và kiểm các tên dưới đây còn đúng:
;   - NSIS_HOOK_POSTUNINSTALL: template gọi bằng `!ifmacrodef` ở CUỐI `Section Uninstall`, sau CheckIfAppIsRunning
;     (app đã đóng) và sau khi xoá file trong $INSTDIR. Gỡ bị huỷ giữa chừng thì không chạy tới đây.
;   - $UpdateMode = 1: chạy với /UPDATE (bộ tự cập nhật, cài đè). Lúc đó không gỡ gì.
;   - $DeleteAppDataCheckboxState = 1: người dùng tích ô "Xoá dữ liệu ứng dụng" ở trang xác nhận gỡ (chạy /S hay /P
;     thì ô này không hiện, biến rỗng = không tích). Template tự xoá $APPDATA\${BUNDLEID} và
;     $LOCALAPPDATA\${BUNDLEID} (dữ liệu WebView2); UTD-Data là thư mục riêng của app (ipc.rs::data_dir) nên dọn ở đây.
;   - Template KHÔNG xoá $INSTDIR đệ quy: chỉ xoá file nó đã chép rồi `RMDir "$INSTDIR"` (thư mục rỗng mới mất).
; File này có chữ Việt nên phải lưu UTF-8 có BOM (makensis đọc file không BOM theo bảng mã ANSI).

; Tên native host = browser_integration.rs::HOST_NAME (cũng là identifier trong tauri.conf.json).
!define UTD_NATIVE_HOST "app.utd.desktop"

; Khoá HKCU mà browser_integration.rs::register_windows ghi. Chỉ đúng khoá tên app.utd.desktop, không đụng khoá khác.
!macro UTD_UNREGISTER_NATIVE_HOST
  DeleteRegKey HKCU "Software\Google\Chrome\NativeMessagingHosts\${UTD_NATIVE_HOST}"
  DeleteRegKey HKCU "Software\Microsoft\Edge\NativeMessagingHosts\${UTD_NATIVE_HOST}"
  DeleteRegKey HKCU "Software\Chromium\NativeMessagingHosts\${UTD_NATIVE_HOST}"
  DeleteRegKey HKCU "Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\${UTD_NATIVE_HOST}"
  DeleteRegKey HKCU "Software\Mozilla\NativeMessagingHosts\${UTD_NATIVE_HOST}"
!macroend

; Manifest native messaging + bản chép native host trong một thư mục dữ liệu. Không xoá gì khác.
; Native host đang chạy (trình duyệt vừa gọi) thì Delete lỗi im lặng; khoá đăng ký đã gỡ nên trình duyệt không gọi lại.
!macro UTD_DELETE_NATIVE_HOST_FILES dir
  Delete "${dir}\native-messaging\chromium.json"
  Delete "${dir}\native-messaging\firefox.json"
  RMDir "${dir}\native-messaging"
  Delete "${dir}\utd-native-host.exe"
  Delete "${dir}\utd-native-host.exe.new"
!macroend

; Dữ liệu app trong một thư mục dữ liệu (UTD-Data, hoặc %LOCALAPPDATA%\UTD của bản cũ, trùng thư mục cài):
; chỉ xoá đúng tên app tạo ra, không bao giờ RMDir /r cả thư mục. Khu cách ly (quarantine) là file người dùng
; đã tải mà Download Guard giữ lại nên KHÔNG xoá (AC-24); tệp lạ cũng giữ, nên các RMDir cuối chỉ xoá thư mục rỗng.
; Thêm tệp mới vào thư mục dữ liệu (ipc.rs::data_dir) thì nhớ thêm tên vào đây.
!macro UTD_DELETE_DATA_FILES dir
  !insertmacro UTD_DELETE_NATIVE_HOST_FILES "${dir}"
  Delete "${dir}\native-host.log"
  Delete "${dir}\utd.db"
  Delete "${dir}\utd.db-wal"
  Delete "${dir}\utd.db-shm"
  Delete "${dir}\utd.db.bak"
  Delete "${dir}\utd.db.partial"
  Delete "${dir}\utd.db.migrating"
  Delete "${dir}\legacy-migrated"
  Delete "${dir}\ipc.json"
  Delete "${dir}\ipc.json.tmp"
  RMDir /r "${dir}\extension"
  RMDir /r "${dir}\bin"
  RMDir /r "${dir}\backup"
  RMDir "${dir}\quarantine"
  RMDir "${dir}"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    ; installMode = currentUser: dữ liệu và khoá đăng ký luôn của người dùng hiện tại. Đặt lại cho chắc vì
    ; perMachine ("all") sẽ biến $LOCALAPPDATA thành C:\ProgramData. Đây là bước cuối của Section nên an toàn.
    SetShellVarContext current

    ${If} $DeleteAppDataCheckboxState = 1
      ; Người dùng chọn xoá dữ liệu: gỡ tích hợp trình duyệt, journal, cài đặt, công cụ media, log. Giữ khu cách ly
      ; và mọi file đã tải: thư mục tải mặc định là Downloads\UTD, commands.rs cấm đặt thư mục tải trong UTD-Data
      ; (AC-24).
      !insertmacro UTD_UNREGISTER_NATIVE_HOST
      ${If} $LOCALAPPDATA != ""
        !insertmacro UTD_DELETE_DATA_FILES "$LOCALAPPDATA\UTD"
        !insertmacro UTD_DELETE_DATA_FILES "$LOCALAPPDATA\UTD-Data"
      ${EndIf}
    ${ElseIf} $EXEDIR != $INSTDIR
      ; Gỡ thường: uninstall.exe tự chép sang %TEMP% rồi chạy từ đó nên $EXEDIR khác $INSTDIR.
      ; Gỡ tích hợp trình duyệt; giữ journal, cài đặt, khu cách ly và mọi file đã tải (AC-24).
      !insertmacro UTD_UNREGISTER_NATIVE_HOST
      !insertmacro UTD_DELETE_NATIVE_HOST_FILES "$LOCALAPPDATA\UTD-Data"
      !insertmacro UTD_DELETE_NATIVE_HOST_FILES "$LOCALAPPDATA\UTD"
    ${EndIf}
    ; Còn lại: $EXEDIR = $INSTDIR nghĩa là bộ cài bản mới đang gỡ bản cũ tại chỗ (`uninstall.exe _?=<thư mục>`,
    ; lựa chọn "Gỡ trước khi cài" ở trang cài lại) rồi cài ngay bản mới: giữ tích hợp trình duyệt để không mất sau
    ; khi nâng cấp.
  ${EndIf}
!macroend
