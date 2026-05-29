//! 定时推送的投递分发
//!
//! 按推送的 `channels`（JSON 数组字符串，如 `["notification"]`）把生成内容投递到各通道。
//! MVP 只实现 `notification`（系统通知）；`popup` / `main_modal` / `daily_note` 留作阶段 2，
//! 命中未实现通道时仅记日志、不报错（保证主链路不被未完成功能阻断）。

use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

use crate::models::PushJob;

/// 解析 channels JSON；失败时回退到 ["notification"]，保证用户至少能收到一次。
fn parse_channels(raw: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(raw).unwrap_or_else(|_| vec!["notification".to_string()])
}

/// 把一次生成结果投递给用户。`content` 为 AI 输出的完整文本。
pub fn dispatch(app: &AppHandle, job: &PushJob, content: &str) {
    let channels = parse_channels(&job.channels);
    for ch in channels {
        match ch.as_str() {
            "notification" => send_notification(app, &job.name, content),
            other => {
                log::info!("[push] 通道 '{}' 暂未实现（job #{}），跳过", other, job.id);
            }
        }
    }
}

/// 执行失败时给用户一声轻提示，避免"没反应"的困惑。仅发系统通知。
pub fn dispatch_failure(app: &AppHandle, job: &PushJob, err: &str) {
    let body: String = format!("生成失败：{}", err).chars().take(200).collect();
    if let Err(e) = app
        .notification()
        .builder()
        .title(format!("📡 {}（失败）", job.name))
        .body(&body)
        .show()
    {
        log::warn!("[push] 失败通知发送也失败了: {}", e);
    }
}

/// 系统通知：标题用推送名，正文用生成内容（截断到 ~300 字，系统通知本身也会截断）。
fn send_notification(app: &AppHandle, name: &str, content: &str) {
    let body: String = content.chars().take(300).collect();
    if let Err(e) = app
        .notification()
        .builder()
        .title(format!("📡 {}", name))
        .body(&body)
        .show()
    {
        log::warn!("[push] 系统通知发送失败: {}", e);
    }
}
