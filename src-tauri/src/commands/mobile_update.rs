//! 移动端"检查更新"（仅 Android/iOS 编译）。
//!
//! 桌面端用 `tauri-plugin-updater` 自动下载+原地替换，但该插件不支持移动端，
//! 且本项目已用 `#[cfg(desktop)]` 把 updater 隔离掉了。移动端没有"原地热替换"
//! 的能力（Android 必须走系统安装器、iOS 必须走 App Store），所以这里只做：
//!
//!   1. 拉同一份 `update.json`（桌面 updater 也读这个）
//!   2. 比对 `version` 字段与当前 App 版本
//!   3. 返回是否有新版本 + 更新说明 + APK 下载 URL
//!
//! 前端拿到结果后弹个对话框，用户点"去下载"就用 `tauri-plugin-opener` 打开
//! APK URL —— 浏览器接管下载，下载完用户点一下，系统安装器接手（首次会引导用户
//! 开"允许安装未知应用"，那是浏览器的权限不是本 App 的，所以 manifest 不用加
//! `REQUEST_INSTALL_PACKAGES`）。
//!
//! `update.json` schema（与 tauri-plugin-updater 约定一致）：
//! ```json
//! {
//!   "version": "1.8.1",
//!   "notes": "更新说明",
//!   "pub_date": "2026-...",
//!   "platforms": {
//!     "windows-x86_64": { "url": "...", "signature": "..." },
//!     "android-arm64":   { "url": "...apk" }   ← 本命令读这个；没有则回落下载页
//!   }
//! }
//! ```

use serde::Serialize;

/// 与桌面 updater 配置（`tauri.conf.json` → `plugins.updater.endpoints`）保持一致，
/// 按顺序尝试，第一个能拿到合法 JSON 的就用。
const UPDATE_JSON_ENDPOINTS: &[&str] = &[
    "https://pub-9d9e6c0cb6934fb0a0c505e3c64f39b2.r2.dev/knowledge-base/update.json",
    "https://gitee.com/bkywksj/knowledge-base-release/raw/master/update.json",
    "https://github.com/bkywksj/knowledge-base-release/raw/main/update.json",
];

/// 当 `update.json` 里没有 `platforms.android-arm64.url` 时，回落到 release 仓库
/// 的发布页，让用户自己挑 APK。
const RELEASE_PAGE_FALLBACK: &str = "https://gitee.com/bkywksj/knowledge-base-release/releases";

#[derive(Debug, Serialize)]
pub struct MobileUpdateInfo {
    pub has_update: bool,
    pub current_version: String,
    pub latest_version: String,
    pub notes: String,
    /// APK 直链（优先）或 release 发布页（回落）
    pub download_url: String,
}

/// 简单版本号比较：把 "1.8.1" 拆成 [1,8,1] 逐段比，b > a 返回 true。
/// 非数字段当 0；段数不同短的补 0。够用了（本项目版本号一直是纯数字三段）。
fn is_newer(latest: &str, current: &str) -> bool {
    let parse = |s: &str| -> Vec<u32> {
        s.trim_start_matches('v')
            .split('.')
            .map(|p| p.trim().parse::<u32>().unwrap_or(0))
            .collect()
    };
    let (a, b) = (parse(current), parse(latest));
    let n = a.len().max(b.len());
    for i in 0..n {
        let ai = a.get(i).copied().unwrap_or(0);
        let bi = b.get(i).copied().unwrap_or(0);
        if bi != ai {
            return bi > ai;
        }
    }
    false
}

/// 拉一个 endpoint 的 update.json，解析失败 / 网络失败都返回 None（让上层试下一个）。
async fn fetch_update_json(url: &str) -> Option<serde_json::Value> {
    let resp = reqwest::Client::new()
        .get(url)
        .header("User-Agent", "knowledge-base-mobile")
        .timeout(std::time::Duration::from_secs(8))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.json::<serde_json::Value>().await.ok()
}

#[tauri::command]
pub async fn check_mobile_update(app: tauri::AppHandle) -> Result<MobileUpdateInfo, String> {
    let current_version = app.package_info().version.to_string();

    // 依次尝试 3 个 endpoint
    let mut json: Option<serde_json::Value> = None;
    for ep in UPDATE_JSON_ENDPOINTS {
        if let Some(v) = fetch_update_json(ep).await {
            json = Some(v);
            break;
        }
    }
    let json = json.ok_or_else(|| "无法连接更新服务器（3 个源都失败），请检查网络".to_string())?;

    let latest_version = json
        .get("version")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "update.json 缺少 version 字段".to_string())?
        .to_string();
    let notes = json
        .get("notes")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // APK 直链：platforms.android-arm64.url > platforms.android-aarch64.url > 回落发布页
    let download_url = json
        .get("platforms")
        .and_then(|p| {
            p.get("android-arm64")
                .or_else(|| p.get("android-aarch64"))
                .or_else(|| p.get("android"))
        })
        .and_then(|entry| entry.get("url"))
        .and_then(|u| u.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| RELEASE_PAGE_FALLBACK.to_string());

    Ok(MobileUpdateInfo {
        has_update: is_newer(&latest_version, &current_version),
        current_version,
        latest_version,
        notes,
        download_url,
    })
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn version_compare() {
        assert!(is_newer("1.8.2", "1.8.1"));
        assert!(is_newer("1.9.0", "1.8.9"));
        assert!(is_newer("2.0.0", "1.99.99"));
        assert!(!is_newer("1.8.1", "1.8.1"));
        assert!(!is_newer("1.8.0", "1.8.1"));
        assert!(is_newer("v1.8.2", "1.8.1")); // 容忍 v 前缀
        assert!(!is_newer("1.8", "1.8.0")); // 段数不同补 0
    }
}
