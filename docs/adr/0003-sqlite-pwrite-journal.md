# ADR-0003 — SQLite WAL + positional writes

## Bối cảnh

File tải có thể 5–50 GB, mạng chập chờn, app có thể bị kill, sleep, rút điện. Cần resume không tải lại và không bao giờ ghép sai file.

## Quyết định

- SQLite WAL làm journal cho task/segment: trạng thái, offset, ETag, kích thước, checksum.
- Ghi file bằng preallocate + `pwrite`/`seek_write` tại offset; không merge file tạm.
- Checkpoint tiến độ có giới hạn tần suất; temp file + rename khi hoàn tất; verify hash trước khi đánh dấu xong.

## Hệ quả

- Resume nhanh, chịu crash/power-cut; cần test chaos và kiểm tra file system/AV lock.
- Windows cần API preallocate/MOTW; SQLite phải có migration và backup trước nâng schema.

## Trạng thái

Accepted
