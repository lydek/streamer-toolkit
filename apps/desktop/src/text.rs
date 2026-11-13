//! Strings shown by the Rust side (tray, native error boxes). zh-TW only for now, kept in one place like the
//! web UI's resource files.

pub const APP_NAME: &str = "Streamer Toolkit";
pub const TRAY_OPEN: &str = "開啟 Streamer Toolkit";
pub const TRAY_QUIT: &str = "結束";

pub fn data_folder_unusable(dir: &str, error: &str) -> String {
    format!(
        "無法使用資料夾：\n{dir}\n\n{error}\n\n請把程式裝在自己可寫入的位置（例如使用者資料夾），或用 --data-dir 指定其他資料夾。"
    )
}
