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

/// Dev builds only; never shown to streamers.
pub fn dev_refuses_folder(dir: &str, flag: &str) -> String {
    format!(
        "開發版不開啟不是由開發版建立的資料夾（可能是正式版的資料）：\n{dir}\n\n開發中的 migration 套用到正式版的資料庫後，正式版會無法開啟它。\n\n確定要用這份資料（例如複製出來的一份）時，加上 {flag}。"
    )
}

pub fn database_unusable(file: &str, error: &str) -> String {
    format!(
        "無法開啟或升級資料庫，程式不會繼續執行：\n{file}\n\n{error}\n\n升級前的備份在資料夾的 backups 裡。"
    )
}

pub const WEBVIEW_MISSING: &str = "找不到 Microsoft Edge WebView2 Runtime，無法開啟管理視窗。\n\nOverlay 仍會繼續運作。按「確定」後會開啟微軟的下載頁面；安裝 Evergreen Runtime 後，在系統匣選「開啟 Streamer Toolkit」即可。";

/// The title the restore notice is found by when it closes itself.
pub const RESTORING_TITLE: &str = "Streamer Toolkit：還原中";
pub const RESTORING: &str =
    "正在還原備份：先把目前的資料備份到資料夾的 backups 裡，再換上備份的資料。

資料很多時需要一點時間，完成後這個視窗會自己關閉。";

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
