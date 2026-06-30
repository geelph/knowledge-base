import { Extension } from "@tiptap/react";
import { Plugin, PluginKey } from "@tiptap/pm/state";
import { Decoration, DecorationSet } from "@tiptap/pm/view";
import type { Node as PMNode } from "@tiptap/pm/model";

export interface WikiLinkOptions {
  /**
   * 点击 wiki 链接时触发。
   * 优先用 `id`（候选下拉选中的稳定锚点，永不失效）；
   * 没有 `id` 时（用户手敲的 `[[标题]]`）回退按 title 查。
   */
  onClick: (title: string, id?: number) => void;
  /**
   * 是否处于阅读模式（不可编辑）。函数式取值，保证编辑器实例只创建一次时仍能拿到实时态。
   * 注：双链点击已统一为「普通单击即跳转」（编辑态/阅读态一致），此项当前不再用于点击门槛，
   * 保留以兼容调用方与未来按模式差异化的需求。
   */
  isReadingMode?: () => boolean;
}

// 识别两种形式：
//   - 旧：[[标题]]
//   - 新：[[标题|123]]  ← 候选下拉选中后插入的形式，ID 为稳定锚点
// `[^\[\]\n|]+` 排除 `|`，让 ID 段独立捕获；ID 必须是纯数字。
const WIKI_LINK_REGEX = /\[\[([^\[\]\n|]+)(?:\|(\d+))?\]\]/g;

function buildDecorations(doc: PMNode): DecorationSet {
  const decorations: Decoration[] = [];
  doc.descendants((node, pos) => {
    if (!node.isText || !node.text) return;
    const text = node.text;
    WIKI_LINK_REGEX.lastIndex = 0;
    let match: RegExpExecArray | null;
    while ((match = WIKI_LINK_REGEX.exec(text)) !== null) {
      const from = pos + match.index;
      const to = from + match[0].length;
      const title = match[1].trim();
      const idStr = match[2]; // 可能为 undefined（旧格式 `[[标题]]`）
      if (!title) continue;

      // 整段 `[[标题|123]]` 加 wiki-link class（含 [[ 和 ]]，方便 Ctrl+点击命中）
      decorations.push(
        Decoration.inline(from, to, {
          class: "wiki-link",
          "data-wiki-link": title,
          ...(idStr ? { "data-wiki-link-id": idStr } : {}),
          title: `点击跳转到「${title}」`,
        }),
      );

      // 带 ID 形式：单独标记 `|123` 段，靠 CSS 视觉隐藏（display:none）。
      // 字符仍在文档里、选中复制时一并带走，仅渲染时不可见 → 视觉上等同 `[[标题]]`。
      if (idStr) {
        // match[0] 形如 `[[标题|123]]`，最后两个 `]]` 占 2 个 char，
        // 倒推：`|123` 段从 `to - 2 - (1 + idStr.length)` 到 `to - 2`
        const pipeFrom = to - 2 - (1 + idStr.length);
        const pipeTo = to - 2;
        decorations.push(
          Decoration.inline(pipeFrom, pipeTo, {
            class: "wiki-link-id-anchor",
          }),
        );
      }
    }
  });
  return DecorationSet.create(doc, decorations);
}

export const WikiLinkDecoration = Extension.create<WikiLinkOptions>({
  name: "wikiLinkDecoration",

  addOptions() {
    return { onClick: () => {}, isReadingMode: () => false };
  },

  addProseMirrorPlugins() {
    const pluginKey = new PluginKey<DecorationSet>("wikiLinkDecoration");
    const onClick = this.options.onClick;

    return [
      new Plugin({
        key: pluginKey,
        state: {
          init: (_, { doc }) => buildDecorations(doc),
          apply: (tr, oldSet) => {
            if (!tr.docChanged) return oldSet.map(tr.mapping, tr.doc);
            return buildDecorations(tr.doc);
          },
        },
        props: {
          decorations(state) {
            return pluginKey.getState(state) ?? DecorationSet.empty;
          },
          // 双链普通单击即跳转（编辑态、阅读态一致）。
          // 历史上编辑态要求 Ctrl/Cmd+点击以保留光标定位，但 .wiki-link 的 cursor:pointer 手型
          // 暗示「可直接点」，普通左键却没反应 → 用户高频反馈「双链点击无效」。改为普通单击即跳。
          // 注：仅点中双链文本本身才跳；点击双链以外位置仍正常定位光标，不影响编辑其它文字。
          handleClick(_view, _pos, event) {
            const target = event.target as HTMLElement | null;
            const el = target?.closest("[data-wiki-link]") as HTMLElement | null;
            if (!el) return false;
            const title = el.getAttribute("data-wiki-link");
            if (!title) return false;
            // 有 ID 锚点优先用 ID（标题改了也能跳到正确笔记）；否则交给上层按 title 查
            const idAttr = el.getAttribute("data-wiki-link-id");
            const id = idAttr ? Number(idAttr) : undefined;
            event.preventDefault();
            onClick(title, Number.isFinite(id) ? id : undefined);
            return true;
          },
        },
      }),
    ];
  },
});
