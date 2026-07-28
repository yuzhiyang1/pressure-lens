use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::{Update, UpdaterExt};

const UPDATE_ENDPOINT: &str =
    "https://github.com/yuzhiyang1/pressure-lens/releases/latest/download/latest.json";
const UPDATE_PUBLIC_KEY: Option<&str> = option_env!("PRESSURE_LENS_UPDATER_PUBKEY");

pub fn is_configured() -> bool {
    UPDATE_PUBLIC_KEY.is_some()
}

/// Updater 下载句柄只保存在 Rust 进程内，避免前端伪造更新地址或签名。
pub struct PendingUpdate(pub Mutex<Option<Update>>);

impl Default for PendingUpdate {
    fn default() -> Self {
        Self(Mutex::new(None))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateMetadata {
    pub version: String,
    pub current_version: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
enum UpdateProgress {
    Started { content_length: Option<u64> },
    Progress { chunk_length: usize },
    Finished,
}

#[tauri::command]
pub async fn check_for_update(
    app: AppHandle,
    pending: State<'_, PendingUpdate>,
) -> Result<Option<UpdateMetadata>, String> {
    let public_key = UPDATE_PUBLIC_KEY
        .ok_or_else(|| "此构建未嵌入更新公钥；请从 GitHub Releases 安装正式签名版本".to_string())?;
    let endpoint = tauri::Url::parse(UPDATE_ENDPOINT).map_err(|error| error.to_string())?;
    let update = app
        .updater_builder()
        .pubkey(public_key)
        .endpoints(vec![endpoint])
        .map_err(|error| error.to_string())?
        .build()
        .map_err(|error| error.to_string())?
        .check()
        .await
        .map_err(|error| error.to_string())?;
    let metadata = update.as_ref().map(|value| UpdateMetadata {
        version: value.version.clone(),
        current_version: value.current_version.clone(),
    });
    *pending
        .0
        .lock()
        .map_err(|_| "更新状态锁不可用".to_string())? = update;
    Ok(metadata)
}

#[tauri::command]
pub async fn install_update(
    app: AppHandle,
    pending: State<'_, PendingUpdate>,
) -> Result<(), String> {
    let update = pending
        .0
        .lock()
        .map_err(|_| "更新状态锁不可用".to_string())?
        .take()
        .ok_or_else(|| "没有待安装更新，请先检查更新".to_string())?;

    let started_app = app.clone();
    let progress_app = app.clone();
    let finished_app = app.clone();
    let mut started = false;
    update
        .download_and_install(
            move |chunk_length, content_length| {
                if !started {
                    let _ = started_app.emit(
                        "update-progress",
                        UpdateProgress::Started { content_length },
                    );
                    started = true;
                }
                let _ =
                    progress_app.emit("update-progress", UpdateProgress::Progress { chunk_length });
            },
            move || {
                let _ = finished_app.emit("update-progress", UpdateProgress::Finished);
            },
        )
        .await
        .map_err(|error| error.to_string())?;

    // Windows 安装器会在安装前自动退出；其他平台等待用户下次手动重启。
    Ok(())
}
