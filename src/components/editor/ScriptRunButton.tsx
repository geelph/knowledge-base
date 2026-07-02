import { useState } from "react";
import { Button, Dropdown, Tooltip, message } from "antd";
import type { MenuProps } from "antd";
import type { Editor } from "@tiptap/react";
import { Braces } from "lucide-react";
import { scriptApi } from "@/lib/api";
import type { Script } from "@/types";

/**
 * #8 Phase 2：编辑器工具栏「脚本」入口。
 * 点开 → 拉取已启用脚本 → 选一个 → 用它转换当前选中文本，输出替换选区。
 * 无选区时提示先选中文本。
 */
export function ScriptRunButton({ editor }: { editor: Editor }) {
  const [scripts, setScripts] = useState<Script[]>([]);
  const [loading, setLoading] = useState(false);
  const [running, setRunning] = useState(false);

  async function loadEnabled() {
    setLoading(true);
    try {
      const all = await scriptApi.list();
      setScripts(all.filter((s) => s.enabled));
    } catch (e) {
      message.error(`加载脚本失败: ${e}`);
    } finally {
      setLoading(false);
    }
  }

  async function runScript(s: Script) {
    const { from, to, empty } = editor.state.selection;
    if (empty) {
      message.warning("请先选中要转换的文本");
      return;
    }
    // 取选中的纯文本（块间用换行分隔，贴近用户视觉）
    const selected = editor.state.doc.textBetween(from, to, "\n");
    setRunning(true);
    try {
      const out = await scriptApi.run(s.id, selected);
      // insertContent 会替换当前选区
      editor.chain().focus().insertContent(out).run();
      message.success(`已应用「${s.name}」`);
    } catch (e) {
      message.error(`脚本执行失败: ${e}`);
    } finally {
      setRunning(false);
    }
  }

  const items: MenuProps["items"] =
    scripts.length === 0
      ? [
          {
            key: "empty",
            disabled: true,
            label: loading ? "加载中…" : "没有已启用的脚本（去设置→脚本插件添加）",
          },
        ]
      : scripts.map((s) => ({
          key: String(s.id),
          label: s.name,
          onClick: () => void runScript(s),
        }));

  return (
    <Dropdown
      trigger={["click"]}
      menu={{ items }}
      onOpenChange={(open) => {
        if (open) void loadEnabled();
      }}
    >
      <Tooltip title="对选中文本运行脚本（Rhai 文本转换）">
        <Button type="text" size="small" icon={<Braces size={16} />} loading={running} />
      </Tooltip>
    </Dropdown>
  );
}
