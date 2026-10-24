/** Chiều cao thanh tiêu đề (px CSS), khớp `h-10` của `TitleBar` và `pt-10` của bố cục. */
const TITLE_BAR_HEIGHT = 40

/**
 * Khoảng chừa với mép khung nhìn cho menu chuột phải, menu "⋯", tooltip (Base UI mặc định 5px mỗi cạnh).
 * Mép trên chừa cả thanh tiêu đề: trên Windows dải 40px trên cùng do chính Windows nhận chuột (vùng kéo
 * `app-region` và cửa sổ con của 3 nút) bất kể lớp DOM, nên phần menu lọt vào đó bấm là kéo cửa sổ hoặc
 * trúng Thu nhỏ/Phóng to/Đóng. Menu không đủ chỗ thì lật hoặc tự thu cao lại (cuộn bên trong).
 */
export const POPUP_COLLISION_PADDING = {
  top: TITLE_BAR_HEIGHT + 4,
  right: 5,
  bottom: 5,
  left: 5,
}
