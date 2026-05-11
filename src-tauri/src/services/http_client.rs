//! 共享 HTTP Client 单例。
//!
//! reqwest::Client 内部维护连接池 + TLS 会话缓存，**必须复用才能避免**
//! 每次请求都重新建立 TCP/TLS 握手的开销。AI 流式回复、WebDAV 同步等
//! 热路径原先每次调用都 `Client::new()`，单次请求多出几百毫秒延迟。
//!
//! 用 `OnceLock` 做进程级单例：
//! - `shared()`：普通用途（OpenAI / Claude / WebDAV 等外网 HTTPS）
//! - `shared_no_proxy()`：真·本地/内网 Ollama（localhost / RFC1918 私网等），绕过系统代理
//! - `shared_ollama_via_proxy()`：部署在 CGNAT / 公网 / 域名 上、需经系统代理隧道才能到达的
//!   Ollama（带同样的 read_timeout）

use std::sync::OnceLock;
use std::time::Duration;

use reqwest::Client;

static SHARED: OnceLock<Client> = OnceLock::new();
static SHARED_NO_PROXY: OnceLock<Client> = OnceLock::new();
static SHARED_OLLAMA_VIA_PROXY: OnceLock<Client> = OnceLock::new();

/// Ollama 流式响应"两次读之间"的空闲超时。
///
/// 用 `read_timeout`（每收到一个 chunk 就重置）而非 `timeout`（整请求总时长）：
/// 流式回复本身可以很长，不能用总时长卡死它；但若 Ollama 卡在加载大模型 / 服务挂死、
/// 长时间一个字都不吐，就应让请求带错误返回，而不是让前端 `send_ai_message` 永不
/// resolve、UI 一直显示"生成中 / 停止"。
const OLLAMA_READ_TIMEOUT: Duration = Duration::from_secs(120);

/// 是否已通过环境变量配置 HTTP 代理（reqwest 默认会自动读这些）。
fn env_proxy_set() -> bool {
    [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
    ]
    .iter()
    .any(|k| std::env::var_os(k).is_some())
}

/// 读 Windows 系统代理设置（注册表 `HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings`），
/// 返回 `host:port`。`ProxyEnable` 为 0 或读不到 → None。
///
/// `ProxyServer` 可能是 `host:port`，也可能是 `http=host:port;https=host:port;...` 形式 ——
/// 后者优先取 `http=` 那段，没有则取第一段。
#[cfg(windows)]
fn windows_system_proxy() -> Option<String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings")
        .ok()?;
    let enabled: u32 = key.get_value("ProxyEnable").ok()?;
    if enabled == 0 {
        return None;
    }
    let server: String = key.get_value("ProxyServer").ok()?;
    let server = server.trim().to_string();
    if server.is_empty() {
        return None;
    }
    if server.contains('=') {
        for part in server.split(';') {
            if let Some(rest) = part.trim().strip_prefix("http=") {
                let rest = rest.trim().to_string();
                if !rest.is_empty() {
                    return Some(rest);
                }
            }
        }
        server
            .split(';')
            .next()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    } else {
        Some(server)
    }
}

#[cfg(not(windows))]
fn windows_system_proxy() -> Option<String> {
    None
}

/// 全局复用的 reqwest Client，自动走系统代理。
pub fn shared() -> &'static Client {
    SHARED.get_or_init(Client::new)
}

/// 不走系统代理的 Client，用于真·本地/内网 Ollama（localhost / 127.x / 192.168.x / 10.x 等）。
///
/// 这类地址本机能直连，强行走 Clash 等系统代理只会被劫持；带 120s `read_timeout`：
/// 避免 Ollama 加载大模型 / 服务挂死时流式请求永远 pending。
pub fn shared_no_proxy() -> &'static Client {
    SHARED_NO_PROXY.get_or_init(|| {
        Client::builder()
            .no_proxy()
            .read_timeout(OLLAMA_READ_TIMEOUT)
            .build()
            .unwrap_or_else(|_| Client::new())
    })
}

/// 走系统代理的 Ollama Client，用于部署在 CGNAT 段（如 Tailscale 的 100.64.0.0/10）/ 公网 IP /
/// 域名 上的 Ollama —— 这类地址往往**只有经系统代理（Clash 等）或 VPN 隧道才能到达**，
/// `shared_no_proxy()` 直连会 `error sending request`（连 TCP 都连不上）。
///
/// 代理来源（按优先级）：
/// 1. `HTTP_PROXY` / `HTTPS_PROXY` / `ALL_PROXY` 等环境变量 —— reqwest 默认会自动读。
/// 2. 环境变量都没设、且在 Windows 上时：补读 **Windows 系统代理**（注册表）。Clash 等
///    "系统代理"开关只写注册表、不设环境变量，reqwest 默认读不到，这里手动补上 ——
///    否则这个"走代理的 Ollama 客户端"实际上没代理可用，照样连不到 CGNAT/公网的 Ollama。
///
/// 同样带 120s `read_timeout`。
pub fn shared_ollama_via_proxy() -> &'static Client {
    SHARED_OLLAMA_VIA_PROXY.get_or_init(|| {
        let mut builder = Client::builder().read_timeout(OLLAMA_READ_TIMEOUT);
        if !env_proxy_set() {
            if let Some(sys) = windows_system_proxy() {
                let url = if sys.contains("://") {
                    sys.clone()
                } else {
                    format!("http://{}", sys)
                };
                match reqwest::Proxy::all(&url) {
                    Ok(proxy) => {
                        builder = builder.proxy(proxy);
                        log::info!("[http] Ollama-via-proxy 客户端启用 Windows 系统代理: {}", url);
                    }
                    Err(e) => {
                        log::warn!("[http] 解析 Windows 系统代理 {:?} 失败: {}", url, e);
                    }
                }
            } else {
                log::info!(
                    "[http] Ollama-via-proxy 客户端：未检测到 HTTP_PROXY 环境变量或 Windows 系统代理 \
                     —— 部署在 CGNAT/公网地址的 Ollama 可能连不上"
                );
            }
        }
        builder.build().unwrap_or_else(|_| Client::new())
    })
}
