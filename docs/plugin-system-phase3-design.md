# 插件系统 Phase 3 设计文档 — 前端 JS 沙箱 UI 插件

> 状态：**设计中（提议）**。本文档只描述方案，**尚未实现**。
> 落地前必须完成本文最后一节的**安全评审清单**与**真机验证**。
>
> 作者：Claude Opus 4.8 协助 · 日期：2026-07-02

---

## 0. 三个 Phase 的定位

| Phase | 机制 | 插件能扩展什么 | 能改 UI | 状态 |
|-------|------|---------------|--------|------|
| **1** | 外部 MCP server（任意语言，独立进程） | 数据 / 自动化 / AI 工具 | ❌ | ✅ 已实现（`services/plugin.rs`） |
| **2** | Rhai 脚本（app 内沙箱） | 文本转换 / 自定义命令 | ❌ | ✅ 已实现（`services/script.rs`） |
| **3** | **前端 JS 沙箱（本文档）** | **面板 / 命令 / 编辑器行为（真 UI）** | ✅ | 🚧 设计中 |

Phase 3 的独有价值 = **真 Obsidian 式 UI 扩展**：插件写 HTML/JS/CSS，渲染成自定义侧栏面板、往工具栏/命令面板加东西。这是**全系统最危险**的一环（运行第三方 UI 代码），因此默认关闭、实验性、需专门安全评审。

---

## 1. 目标 / 非目标

### 目标（Phase 3a MVP）
- 用户能写一个"面板插件"：一段 HTML/JS/CSS，渲染成一个**侧栏面板**。
- 插件能通过**受限的桥 API** 跟知识库交互（MVP 只给**只读**：搜索、读当前笔记、发通知）。
- 插件运行在**隔离沙箱**里，碰不到主窗口的 DOM / 存储 / Tauri IPC / 网络。
- 插件默认**禁用**，安装/启用时明确告知"这是运行第三方代码"。

### 非目标（本期不做，留给 3b/3c）
- 插件直接操作编辑器 ProseMirror 文档（太危险，需要更细的 API 面）。
- 插件市场 / 远程安装 / 自动更新。
- 插件写笔记 / 删数据（写能力要在能力模型成熟后再逐步开）。
- 插件访问文件系统 / 网络（一律不给；要联网让 Phase 1 的 MCP 插件去做）。

---

## 2. 威胁模型（我们在防什么）

假设**插件代码是恶意的**（用户从网上抄了一段插件）。必须防住：

| 威胁 | 后果 | 缓解 |
|------|------|------|
| 插件读主窗口 DOM / localStorage / cookie | 偷笔记、偷会话 | 沙箱 iframe **不给 `allow-same-origin`** → 唯一不透明源，碰不到父窗口 |
| 插件拿到 `window.__TAURI__` 调 invoke | 直接调所有 Rust 命令 = 完全失控 | 同上，隔离源里没有 `__TAURI__`；且桥只转发白名单能力 |
| 插件 fetch 外部服务器（偷数据/C2） | 数据外泄 | iframe 无网络（sandbox 不给，CSP 不放行）；桥不提供网络能力 |
| 插件伪造 postMessage 冒充别的插件/宿主 | 越权调用 | 宿主校验 `event.source === iframe.contentWindow`、校验 `event.origin`、每个 iframe 带唯一 nonce |
| 插件死循环 / 狂发消息 DoS | 卡死 UI | iframe 在独立事件循环（不完全隔离但有限）；宿主对消息限流；提供"强制卸载"按钮 |
| 沙箱逃逸（浏览器引擎漏洞） | 全失控 | 无法从代码层根治——靠"默认禁用+用户明确授信+WebView2/WKWebView 及时更新"，并在文档里声明残余风险 |
| CSP 绕过（inline script 注入主窗口） | XSS | 主窗口 CSP 保持 `script-src 'self'`；插件内容**只进 iframe**，绝不 `innerHTML` 进主窗口 |

**核心原则**：插件与主应用之间**只有一条通道**（postMessage 桥），桥的每一端都不信任对方、都做校验。

---

## 3. 沙箱机制（方案 A · iframe）

### 3.1 为什么是 iframe
Web Worker / QuickJS 都**渲染不了任意 HTML**（无 DOM），做不到"真 UI 面板"。只有 iframe 能。代价是它是三者里攻击面最大的，用上面的威胁模型缓解。

### 3.2 隔离配置（关键）
```html
<iframe
  sandbox="allow-scripts"      <!-- ✅ 只给脚本；❌ 绝不加 allow-same-origin -->
  src="<plugin blob url>"       <!-- 见 3.3 -->
  csp="...">                    <!-- 见 3.4 -->
</iframe>
```
- **`allow-scripts` 且不给 `allow-same-origin`** → iframe 是**唯一不透明源**（origin = `null`）。它：
  - 碰不到父文档 DOM（跨源）
  - 碰不到 `localStorage` / `cookie`（不透明源无存储访问）
  - 拿不到 `window.parent.__TAURI__`（跨源）
  - 只能靠 `window.parent.postMessage` 跟宿主说话
- ⚠️ **铁律**：`allow-scripts` + `allow-same-origin` **同时给 = 沙箱失效**（iframe 能 reach 回父窗口）。**永远不给 allow-same-origin。**

### 3.3 插件内容怎么进 iframe：`blob:` URL（不是 srcdoc）
**关键坑**：如果用 `srcdoc="<html>"`，很多引擎会让 iframe 内容**继承父文档的 CSP**（`script-src 'self'`）→ **插件的 inline script 被 CSP 直接 block**，插件跑不起来。

**方案**：把插件 HTML 拼好后 `new Blob([html], {type:'text/html'})` → `URL.createObjectURL(blob)` 得到 `blob:` URL → `iframe.src = blobUrl`。`blob:` 文档有自己的源 + 自己的（默认较宽松）CSP，inline script 能跑。用完 `URL.revokeObjectURL` 释放。

> 🔴 **必须 dev 验证**：`blob:` iframe 在 Tauri WebView2 / WKWebView 下能否加载 + 跑 inline script。若不行，退路是"自定义 asset 协议提供插件页"或"独立 WebviewWindow"。

### 3.4 CSP 改动（需要，且需验证）
当前主窗口 CSP：`frame-src 'self' asset: <视频域...>`——**不含 `blob:`**。要让 `blob:` iframe 能被嵌入，需给 `frame-src` 加 `blob:`：
```
frame-src 'self' asset: blob: <原有视频域...>
```
- 只加 `frame-src blob:`，**不动** `script-src 'self'`（主窗口仍禁 inline，插件 inline 只在 iframe 内、受 iframe 自己的 CSP 管）。
- 可选：给 iframe 自身再设一层严格 CSP（`default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'`）——**禁掉插件 iframe 的一切网络**（`connect-src` / `img-src` 外部全禁），只留 inline 脚本/样式跑。这样插件连图片外链都发不出去，进一步堵数据外泄。
- 🔴 **必须 dev 验证**：加 `blob:` 后主窗口自身安全性无回归；iframe 内 CSP 生效。

---

## 4. 桥协议（postMessage）

### 4.1 握手
1. 宿主创建 iframe 时生成一次性 `nonce`（`crypto.randomUUID()`），注入进插件 HTML（作为全局 `KB_NONCE`）。
2. iframe 加载后向父窗口 `postMessage({type:'kb:ready', nonce})`。
3. 宿主校验：`event.source === iframe.contentWindow` **且** `msg.nonce === 预期 nonce`。通过才认这个 iframe。
4. 之后每条消息都带 `nonce` + 递增 `seq`；宿主两端都校验。

### 4.2 消息 schema
```ts
// 插件 → 宿主（请求）
interface PluginRequest {
  kb: true;                     // 魔法字段，快速甄别
  nonce: string;                // 握手 nonce
  seq: number;                  // 请求序号，用于匹配响应
  cap: string;                  // 能力名，如 "search" / "getActiveNote" / "notify"
  args?: unknown;               // 能力入参（宿主侧再严格校验类型）
}
// 宿主 → 插件（响应）
interface HostResponse {
  kb: true;
  nonce: string;
  seq: number;                  // 对应请求
  ok: boolean;
  data?: unknown;               // ok=true 时的结果
  error?: string;               // ok=false 时的错误
}
```

### 4.3 宿主侧校验（每条消息，缺一不可）
```
onmessage(event):
  1. event.source === thisIframe.contentWindow   // 防别的窗口伪造
  2. msg.kb === true && msg.nonce === expectedNonce  // 防跨插件伪造
  3. msg.cap ∈ 该插件被授予的能力白名单            // 能力门控
  4. 按 cap 严格校验 args 类型/长度/范围            // 不信任入参
  5. 调对应宿主 handler（handler 内部再走既有 API，带自己的校验）
  6. 回 postMessage(iframe, response, targetOrigin='*')  // 不透明源只能用 '*'，靠 nonce 保真
```

---

## 5. 能力模型（MVP 只给只读）

每个插件在其 manifest 里声明需要的能力；用户启用时看到能力清单并授权。宿主只转发**已授权 + 在白名单内**的能力。

### Phase 3a（MVP）能力集（全只读，低危）
| cap | 说明 | 底层复用 | 风险 |
|-----|------|---------|------|
| `search(kw, limit?)` | 全文搜索笔记，返回 `{id,title,snippet}[]` | `searchApi` | 低（只读） |
| `getActiveNote()` | 当前打开笔记的 `{id,title,content}` | 编辑器/tabs store | 中（暴露正文给插件，需用户知情） |
| `notify(msg, level?)` | 弹一条 antd message | `message.*` | 低 |
| `getTheme()` | 当前主题（light/dark）+ 少量令牌，供插件配色 | store | 无 |

### 未来（3b+，需逐个安全评审后开）
`createNote` / `updateNote`（写）、`listTags`、`getSelection` / `replaceSelection`（要先给编辑器 API）、`openNote(id)`（导航）、`storage.get/set`（插件私有 KV，隔离命名空间）。

> **能力升级铁律**：任何**写**能力或**导航**能力，都必须单独评审，且默认关，用户逐项授权。MVP 阶段一律不开写能力。

---

## 6. 数据模型

复用 Phase 2 的 `scripts` 表模式思路，但 UI 插件字段更多，建议**新表**：

```sql
-- schema.rs migrate_v48_to_v49（示意）
CREATE TABLE IF NOT EXISTS ui_plugins (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    version     TEXT NOT NULL DEFAULT '1.0.0',
    author      TEXT NOT NULL DEFAULT '',
    -- 插件本体：三段代码分开存，宿主拼装进 iframe
    html        TEXT NOT NULL DEFAULT '',
    js          TEXT NOT NULL DEFAULT '',
    css         TEXT NOT NULL DEFAULT '',
    -- 声明请求的能力（JSON 字符串数组，如 ["search","notify"]）
    capabilities TEXT NOT NULL DEFAULT '[]',
    -- 挂载点：当前只支持 "panel"（侧栏面板）
    mount       TEXT NOT NULL DEFAULT 'panel',
    enabled     INTEGER NOT NULL DEFAULT 0,   -- 🔴 默认禁用
    created_at  DATETIME DEFAULT (datetime('now','localtime')),
    updated_at  DATETIME DEFAULT (datetime('now','localtime'))
);
```

Rust：`database/ui_plugins.rs`（DAO）+ `models::UiPlugin/UiPluginInput` + `commands/ui_plugin.rs`（CRUD，纯存取，**不执行任何代码**——执行全在前端 iframe）。安装/脚手架可复用 Phase 1 的清单思路（`kb-ui-plugin.json`）。

---

## 7. 前端架构

```
src/
├── lib/plugins/
│   ├── sandboxHost.ts        # iframe 生命周期 + postMessage 桥 + 能力分发（核心，最需评审）
│   ├── capabilities.ts       # 每个 cap 的 handler + args 校验（只读实现）
│   └── pluginHtml.ts         # 把 html/js/css + nonce + iframe-CSP 拼成完整文档
├── components/plugins/
│   ├── SandboxedPluginPanel.tsx  # 宿主 React 组件：渲染 iframe，接 sandboxHost
│   └── PluginPanelHost.tsx       # 在侧栏挂载已启用的 panel 插件（tab 切换）
├── components/settings/
│   └── UiPluginSection.tsx   # 设置页 CRUD + 启用开关 + 能力授权 + 实验警告
```

### 7.1 挂载点
- 复用现有 **SidePanel** 机制（`AppLayout.tsx` 的 `SidePanel` 浮层 + `ActivityBar`）。给 ActivityBar 加一个"插件"图标，点开在 SidePanel 里渲染 `PluginPanelHost`，内部按启用的插件出 tab，每个 tab 是一个 `SandboxedPluginPanel`（一个 iframe）。
- **不**往编辑器/工具栏注入（3a 不碰编辑器 DOM，降风险）。

### 7.2 pluginHtml 拼装（示意）
```
<!doctype html><html><head>
  <meta http-equiv="Content-Security-Policy"
        content="default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data:">
  <style>${css}</style>
</head><body>
  ${html}
  <script>
    const KB_NONCE = ${JSON.stringify(nonce)};
    // 注入极简 SDK：kb.search()/kb.getActiveNote()/kb.notify()，内部走 postMessage + seq 匹配
    const kb = (function(){ /* seq 计数 + Promise 表 + onmessage 分发 */ })();
    // 用户 js
    ${js}
  </script>
</body></html>
```
> iframe 内 CSP `default-src 'none'` + 只放 inline script/style → 插件**无法联网、无法加载外部资源**，图片只能 `data:`。这是第二道数据外泄防线。

---

## 8. 插件 manifest（安装用，复用 Phase 1 思路）

```json
{
  "manifestVersion": 1,
  "name": "我的面板插件",
  "description": "示例",
  "version": "1.0.0",
  "author": "",
  "mount": "panel",
  "capabilities": ["search", "notify"],
  "html": "index.html",
  "js": "plugin.js",
  "css": "style.css"
}
```
安装命令 `ui_plugin_install_from_file(path)`：读目录里的 html/js/css + manifest → 建 `ui_plugins` 行（`enabled=0`）。脚手架 `ui_plugin_scaffold(dir,name)` 生成一个"计数器面板"示例。

---

## 9. 插件示例（作者视角）

`plugin.js`：
```js
// SDK 由宿主注入为全局 kb
document.getElementById('go').onclick = async () => {
  const kw = document.getElementById('kw').value;
  const hits = await kb.search(kw, 10);      // 只读能力
  document.getElementById('out').textContent =
    hits.map(h => `#${h.id} ${h.title}`).join('\n');
  kb.notify(`搜到 ${hits.length} 条`);
};
```
`index.html`：`<input id=kw><button id=go>搜</button><pre id=out></pre>`

体验：设置→UI 插件→装入/新建→启用（弹能力授权+第三方代码警告）→ 活动栏"插件"→ 侧栏出现这个面板，能搜知识库。

---

## 10. 实施分期

| 期 | 内容 | 风险 |
|----|------|------|
| **3a（MVP）** | iframe 沙箱宿主 + 桥 + 3~4 个只读能力 + panel 挂载 + 设置 CRUD（默认禁用） | 需真机验证 blob/CSP + 安全评审 |
| **3b** | 写能力（createNote/updateNote，逐项授权）+ 插件私有 storage（隔离 KV）+ 命令注册（往命令面板加插件命令） | 每个写能力单独评审 |
| **3c** | 编辑器 API（getSelection/replaceSelection/装饰）+ 更多挂载点（工具栏/状态栏）+ 插件市场 | 编辑器 API 面大，最高风险 |

---

## 11. 🔴 落地前必做：安全评审清单

实现完 3a 后、**给用户开放前**，必须逐项验证：

- [ ] **真机验证** `blob:` 沙箱 iframe 在 Windows WebView2 / macOS WKWebView / Linux WebKitGTK 下能加载并跑 inline script。
- [ ] iframe **确未**拿到 `allow-same-origin`；`window.parent.__TAURI__` 在 iframe 内为 undefined。
- [ ] iframe 内 `fetch('https://evil.com')` **被 CSP 拒**；`localStorage` 访问抛错或为空。
- [ ] 主窗口 CSP 加 `frame-src blob:` 后，**主窗口自身**仍禁 inline script（`script-src 'self'` 未松动）；无 XSS 回归。
- [ ] postMessage 校验：伪造 `nonce` / 伪造 `source` 的消息被丢弃；未授权 `cap` 被拒。
- [ ] 能力 handler 的 args 校验：超长/畸形/注入型入参不会打穿底层 API。
- [ ] DoS：插件狂发消息时宿主限流不卡死；"强制卸载/禁用"按钮可用。
- [ ] `getActiveNote` 暴露正文给插件——UI 上对用户**明确告知**并需授权。
- [ ] 插件默认 `enabled=0`；启用弹"运行第三方代码"警告。
- [ ] 卸载/禁用插件时 iframe 被销毁、`revokeObjectURL`、桥监听被移除，无泄漏。
- [ ] （建议）请安全背景的人 review `sandboxHost.ts` + `capabilities.ts` + CSP 改动。

---

## 12. 残余风险声明

即使以上全做到，**运行第三方 UI 代码本身**仍有不可完全消除的风险（WebView 引擎 0day 逃逸）。因此：
- Phase 3 永远是**实验性、默认关闭、opt-in**。
- 文档/UI 明确告知用户"只装你信任来源的插件"。
- 不做"一键从网上装插件"这种降低信任门槛的功能（至少 3a/3b 不做）。

---

## 13. 与已实现部分的关系

- 复用 Phase 1（`services/plugin.rs`）的**清单一键安装 + 脚手架**思路（换成 UI 插件的 manifest/字段）。
- 复用 Phase 2（`services/script.rs`）的**设置页 CRUD + 启用开关**模式。
- 复用现有 **SidePanel / ActivityBar** 布局做面板挂载点。
- 桥的只读能力**复用现有前端 API**（`searchApi` / tabs store / `message`），不新增 Rust 命令（除 `ui_plugins` 的 CRUD）。

> 一句话：Phase 3a 后端很轻（就一张表 + CRUD），**难点 100% 在前端沙箱宿主 + 桥 + CSP**，且这部分**必须真机验证 + 安全评审**才能开放。
