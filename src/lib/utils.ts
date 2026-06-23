/** 去除 HTML 标签，提取纯文本
 *
 * 用正则替代 DOMParser：DOMParser 对 50KB HTML 需 20-50ms，正则只需 1-5ms。
 * 笔记列表（50+ 条）+ PDF/Word 抽出的大 content 场景下显著优化。
 */
export function stripHtml(html: string): string {
  if (!html) return "";
  return html
    .replace(/<style[^>]*>[\s\S]*?<\/style>/gi, "")
    .replace(/<script[^>]*>[\s\S]*?<\/script>/gi, "")
    .replace(/<[^>]+>/g, "")
    .replace(/&nbsp;/g, " ")
    .replace(/&amp;/g, "&")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/\s+/g, " ")
    .trim();
}

/** 本地时区的 YYYY-MM-DD。
 *
 * 🔴 全前端「某天 → 日期串」的唯一真相源。严禁再用
 * `new Date().toISOString().slice(0,10)` —— 那取的是 UTC 日期，与后端
 * （daily 用 `chrono::Local`，统计用 `DATE(updated_at,'localtime')`）口径不一致：
 * 东八区本地 00:00–08:00 会差一天，导致同一个「今天」在不同入口被算成两条日记
 * （日记重复增殖），以及写作热力图/连续天数错位。前端必须对齐到本地。
 */
export function localYmd(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

/** 本地时区「今天」的 YYYY-MM-DD（见 {@link localYmd} 的口径说明）。 */
export function todayYmd(): string {
  return localYmd(new Date());
}

/** 相对时间格式化 */
export function relativeTime(dateStr: string): string {
  const date = new Date(dateStr);
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffMin = Math.floor(diffMs / 60000);
  if (diffMin < 1) return "刚刚";
  if (diffMin < 60) return `${diffMin}分钟前`;
  const diffHour = Math.floor(diffMin / 60);
  if (diffHour < 24) return `${diffHour}小时前`;
  const diffDay = Math.floor(diffHour / 24);
  if (diffDay < 7) return `${diffDay}天前`;
  return dateStr.slice(0, 10);
}
