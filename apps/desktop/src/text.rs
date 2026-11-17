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

pub fn database_unusable(file: &str, error: &str) -> String {
    format!(
        "無法開啟或升級資料庫，程式不會繼續執行：\n{file}\n\n{error}\n\n升級前的備份在資料夾的 backups 裡。"
    )
}

pub const WEBVIEW_MISSING: &str = "找不到 Microsoft Edge WebView2 Runtime，無法開啟管理視窗。\n\nOverlay 仍會繼續運作。按「確定」後會開啟微軟的下載頁面；安裝 Evergreen Runtime 後，在系統匣選「開啟 Streamer Toolkit」即可。";

pub const RESTORED: &str = "已還原備份。

備份不含 Twitch 登入和 OBS 密碼：需要時請到設定頁重新登入 Twitch、重填 OBS 密碼。

還原前的資料已備份到資料夾的 backups 裡。";

pub fn restore_failed(error: &str) -> String {
    format!(
        "無法還原備份，目前的資料沒有變動：

{error}"
    )
}

pub fn restore_marker_left(dir: &str, error: &str) -> String {
    format!(
        "還原已結束，但無法刪除資料夾裡的 restore-pending 檔：
{dir}

{error}

請關閉可能正在使用這個資料夾的程式後手動刪除它。在那之前，每次啟動都會提示沒有可還原的備份，現在的資料不會變動。"
    )
}

pub fn restore_put_back_failed(dir: &str, error: &str) -> String {
    format!(
        "還原備份失敗，也無法放回原本的資料，程式不會繼續執行：
{dir}

{error}

原本的資料在資料夾的 restore-previous 裡，也備份在 backups 裡。請關閉可能正在使用這個資料夾的程式後再啟動，程式會再試一次放回原本的資料。"
    )
}
