/**
 * IDEA 风格的"对比 / 合并"弹窗：左右两栏 CodeMirror MergeView，中缝带 ▶（把左侧变更块覆盖到右侧）。
 *
 * 约定：**右侧 = 最终结果**。
 *  - 剪贴板对比：左 = 剪贴板（只读），右 = 当前笔记 markdown（可编辑），▶ 把剪贴板的块拉进笔记
 *  - 笔记 vs 笔记：左 = 另一篇（可编辑），右 = 当前/目标笔记（可编辑），▶ 把另一篇的块拉进目标
 *
 * 保存：onSave 提供时右下角出现「保存更改」，回调拿到两侧编辑后的最终文本，由调用方决定怎么写回。
 *
 * 实现要点：
 *  1. MergeView 是命令式 DOM 库，需要一个已挂载且**有固定高度**的容器 —— 用 callback ref 在 div 真正挂进
 *     DOM 那一刻创建（避免 antd Modal 内容异步挂载导致 useEffect 里 ref 还是 null、整片空白）。
 *  2. CodeMirror 的 MergeView 不自动同步两侧滚动 —— 这里手动监听两个 .cm-scroller 的 scroll，按比例镜像
 *     scrollTop（带防抖锁防回环），并提供「同步滚动」开关。
 *  3. 两侧文本先把 \r\n 归一成 \n，否则一边带 \r、一边不带会被判成"整篇每行都变了"。
 */
import { useCallback, useRef, useState } from "react";
import { Alert, Button, Modal, Space, Switch } from "antd";
import { MergeView } from "@codemirror/merge";
import { EditorView, lineNumbers } from "@codemirror/view";
import { EditorState } from "@codemirror/state";
import { markdown } from "@codemirror/lang-markdown";
import { useAppStore } from "@/store";

export interface DiffSide {
  label: string;
  value: string;
  editable: boolean;
}

interface Props {
  open: boolean;
  onClose: () => void;
  left: DiffSide;
  right: DiffSide;
  /** 提供则右下角显示「保存更改」按钮；回调拿到两侧编辑后的最终文本 */
  onSave?: (result: { left: string; right: string }) => Promise<void> | void;
  /** 「保存更改」下方的小字警告（如"将以 markdown 重新生成笔记内容，自定义块可能丢失"） */
  saveHint?: string;
}

const normalizeEol = (s: string) => s.replace(/\r\n/g, "\n");

// CM 主题：让编辑器填满（高度由外层 host div 固定）
const fillTheme = EditorView.theme({
  "&": { height: "100%" },
  ".cm-scroller": { overflow: "auto" },
});
const darkTheme = EditorView.theme(
  {
    "&": { backgroundColor: "transparent", color: "var(--ant-color-text, #ddd)" },
    ".cm-gutters": {
      backgroundColor: "transparent",
      color: "#888",
      borderRight: "1px solid rgba(255,255,255,0.08)",
    },
    ".cm-activeLine": { backgroundColor: "rgba(255,255,255,0.04)" },
    ".cm-activeLineGutter": { backgroundColor: "rgba(255,255,255,0.06)" },
    ".cm-selectionBackground, .cm-content ::selection": {
      backgroundColor: "rgba(80,150,255,0.30)",
    },
    ".cm-cursor": { borderLeftColor: "#ddd" },
  },
  { dark: true },
);
const lightTheme = EditorView.theme({
  "&": { backgroundColor: "transparent", color: "var(--ant-color-text, #222)" },
  ".cm-gutters": {
    backgroundColor: "transparent",
    color: "#aaa",
    borderRight: "1px solid rgba(0,0,0,0.06)",
  },
});

function sideExtensions(editable: boolean, dark: boolean) {
  return [
    lineNumbers(),
    EditorView.lineWrapping,
    markdown(),
    fillTheme,
    dark ? darkTheme : lightTheme,
    EditorView.editable.of(editable),
    ...(editable ? [] : [EditorState.readOnly.of(true)]),
  ];
}

/** 两侧 .cm-scroller 按比例镜像 scrollTop；返回清理函数 */
function linkScrollers(a: HTMLElement, b: HTMLElement, enabledRef: React.MutableRefObject<boolean>) {
  let lock = false;
  const mirror = (src: HTMLElement, dst: HTMLElement) => {
    if (!enabledRef.current || lock) return;
    lock = true;
    const srcMax = src.scrollHeight - src.clientHeight;
    const dstMax = dst.scrollHeight - dst.clientHeight;
    dst.scrollTop = srcMax > 0 ? (src.scrollTop / srcMax) * dstMax : 0;
    requestAnimationFrame(() => {
      lock = false;
    });
  };
  const onA = () => mirror(a, b);
  const onB = () => mirror(b, a);
  a.addEventListener("scroll", onA, { passive: true });
  b.addEventListener("scroll", onB, { passive: true });
  return () => {
    a.removeEventListener("scroll", onA);
    b.removeEventListener("scroll", onB);
  };
}

export function DiffMergeModal({ open, onClose, left, right, onSave, saveHint }: Props) {
  const dark = useAppStore((s) => s.themeCategory) === "dark";
  const mvRef = useRef<MergeView | null>(null);
  const unlinkRef = useRef<(() => void) | null>(null);
  // callback ref 的 [] 依赖闭包读不到最新 props，用 ref 兜住
  const latest = useRef({ left, right, dark });
  latest.current = { left, right, dark };
  const [saving, setSaving] = useState(false);
  const [syncScroll, setSyncScroll] = useState(true);
  const syncScrollRef = useRef(true);
  syncScrollRef.current = syncScroll;

  const teardown = () => {
    unlinkRef.current?.();
    unlinkRef.current = null;
    mvRef.current?.destroy();
    mvRef.current = null;
  };

  // div 挂载 → 创建 MergeView + 装滚动同步；卸载（destroyOnClose）→ 全部销毁
  const setHostEl = useCallback((el: HTMLDivElement | null) => {
    teardown();
    if (!el) return;
    const { left, right, dark } = latest.current;
    const mv = new MergeView({
      a: { doc: normalizeEol(left.value), extensions: sideExtensions(left.editable, dark) },
      b: { doc: normalizeEol(right.value), extensions: sideExtensions(right.editable, dark) },
      parent: el,
      orientation: "a-b",
      revertControls: "a-to-b", // 中缝 ▶：把左(a)的变更块覆盖到右(b)。右侧 = 最终结果。
      highlightChanges: true,
      gutter: true,
      collapseUnchanged: { margin: 3, minSize: 6 },
    });
    mvRef.current = mv;
    // 等一帧让 DOM 布局完成再装滚动监听
    requestAnimationFrame(() => {
      if (mvRef.current !== mv) return;
      unlinkRef.current = linkScrollers(mv.a.scrollDOM, mv.b.scrollDOM, syncScrollRef);
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function handleSave() {
    if (!onSave || !mvRef.current) return;
    const leftDoc = mvRef.current.a.state.doc.toString();
    const rightDoc = mvRef.current.b.state.doc.toString();
    setSaving(true);
    try {
      await onSave({ left: leftDoc, right: rightDoc });
      onClose();
    } catch (e) {
      console.error("[DiffMergeModal] onSave 失败:", e);
    } finally {
      setSaving(false);
    }
  }

  return (
    <Modal
      open={open}
      onCancel={onClose}
      destroyOnClose
      title={`${left.label}  ↔  ${right.label}`}
      width="92vw"
      style={{ top: 16, maxWidth: 1400 }}
      styles={{ body: { paddingTop: 8 } }}
      footer={
        <Space>
          <Button onClick={onClose}>取消</Button>
          {onSave && (
            <Button type="primary" loading={saving} onClick={handleSave}>
              保存更改
            </Button>
          )}
        </Space>
      }
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          fontSize: 12,
          color: "var(--ant-color-text-secondary, #888)",
          marginBottom: 6,
        }}
      >
        <span>
          左 = {left.label}
          {left.editable ? "" : "（只读）"}，右 = {right.label}
          {right.editable ? "" : "（只读）"}。中缝 ▶ 把左侧变更块覆盖到右侧；两栏均可直接编辑。
        </span>
        <span style={{ display: "inline-flex", alignItems: "center", gap: 6, flexShrink: 0 }}>
          <span>同步滚动</span>
          <Switch size="small" checked={syncScroll} onChange={setSyncScroll} />
        </span>
      </div>
      <div
        ref={setHostEl}
        style={{
          height: "64vh",
          display: "flex",
          flexDirection: "column",
          border: "1px solid var(--ant-color-border-secondary, #eee)",
          borderRadius: 6,
          overflow: "hidden",
        }}
      />
      {saveHint && onSave && (
        <Alert type="warning" showIcon banner style={{ marginTop: 8 }} message={saveHint} />
      )}
    </Modal>
  );
}
