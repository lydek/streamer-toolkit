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

pub const WEBVIEW_MISSING: &str = "找不到 Microsoft Edge WebView2 Runtime，無法開啟管理視窗。\n\nOverlay 仍會繼續運作。按「確定」後會開啟微軟的下載頁面；安裝 Evergreen Runtime 後，在系統匣選「開啟 Streamer Toolkit」即可。";
