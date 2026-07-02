import { useEffect, useState } from "react";
import {
  Card,
  Typography,
  Button,
  Space,
  Table,
  Switch,
  Modal,
  Form,
  Input,
  Alert,
  message,
  Empty,
  Popconfirm,
} from "antd";
import { PlusOutlined, PlayCircleOutlined } from "@ant-design/icons";
import { Trash2, Pencil } from "lucide-react";
import { scriptApi } from "@/lib/api";
import type { Script, ScriptInput } from "@/types";

const { Text, Paragraph } = Typography;

/** 内置示例脚本，帮用户快速上手 Rhai 文本转换语法 */
const EXAMPLE_CODE = `// 脚本拿到选中文本变量 input（字符串），返回值 = 最后一个表达式。
// Rhai 语法接近 JS + Rust。字符串方法里 to_upper/to_lower/+ 拼接是「返回新串」，
// 而 trim()/replace()/make_upper() 是「原地修改、返回 unit」——原地改后要把变量作为末表达式返回。

// 示例：去首尾空格 + 全部大写
let s = input;
s.trim();
s.to_upper()`;

const DEFAULT_INPUT: ScriptInput = {
  name: "",
  description: "",
  kind: "transform",
  trigger: "selection",
  code: EXAMPLE_CODE,
  enabled: true,
};

/**
 * #8 Phase 2 脚本插件管理。脚本 = 一段 Rhai 文本转换代码（沙箱执行，无文件/网络访问）。
 * 在编辑器里选中文本 → 工具栏「脚本」菜单选一个已启用脚本 → 用其输出替换选区。
 */
export function ScriptSection() {
  const [scripts, setScripts] = useState<Script[]>([]);
  const [loading, setLoading] = useState(false);
  const [modalOpen, setModalOpen] = useState(false);
  const [editingId, setEditingId] = useState<number | null>(null);
  const [form] = Form.useForm<ScriptInput>();
  // 试运行
  const [testInput, setTestInput] = useState("  hello world  ");
  const [testOutput, setTestOutput] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);

  useEffect(() => {
    void load();
  }, []);

  async function load() {
    setLoading(true);
    try {
      setScripts(await scriptApi.list());
    } catch (e) {
      message.error(`加载脚本失败: ${e}`);
    } finally {
      setLoading(false);
    }
  }

  function openCreate() {
    setEditingId(null);
    form.setFieldsValue(DEFAULT_INPUT);
    setTestOutput(null);
    setModalOpen(true);
  }

  function openEdit(s: Script) {
    setEditingId(s.id);
    form.setFieldsValue({
      name: s.name,
      description: s.description,
      kind: s.kind,
      trigger: s.trigger,
      code: s.code,
      enabled: s.enabled,
    });
    setTestOutput(null);
    setModalOpen(true);
  }

  async function handleTest() {
    setTesting(true);
    setTestOutput(null);
    try {
      const code = form.getFieldValue("code") ?? "";
      const out = await scriptApi.runPreview(code, testInput);
      setTestOutput(out);
    } catch (e) {
      setTestOutput(`❌ ${e}`);
    } finally {
      setTesting(false);
    }
  }

  async function handleSave() {
    try {
      const v = await form.validateFields();
      const input: ScriptInput = {
        name: v.name,
        description: v.description ?? "",
        kind: "transform",
        trigger: v.trigger ?? "selection",
        code: v.code ?? "",
        enabled: v.enabled ?? true,
      };
      if (editingId === null) {
        await scriptApi.create(input);
        message.success("已创建脚本");
      } else {
        await scriptApi.update(editingId, input);
        message.success("已保存脚本");
      }
      setModalOpen(false);
      void load();
    } catch (e) {
      // validateFields 抛的是校验错误对象，其它才是真错误
      if (e && typeof e === "object" && "errorFields" in e) return;
      message.error(`保存失败: ${e}`);
    }
  }

  async function handleDelete(id: number) {
    try {
      await scriptApi.delete(id);
      message.success("已删除");
      void load();
    } catch (e) {
      message.error(`删除失败: ${e}`);
    }
  }

  async function handleToggle(s: Script, enabled: boolean) {
    try {
      await scriptApi.setEnabled(s.id, enabled);
      void load();
    } catch (e) {
      message.error(`切换失败: ${e}`);
    }
  }

  return (
    <Card id="settings-scripts" title="脚本插件（文本转换）">
      <Alert
        type="info"
        showIcon
        className="mb-3"
        message="脚本 = 一段 Rhai 代码，接收选中文本 input、返回转换结果。沙箱执行（无文件/网络访问、有资源上限）。在编辑器里选中文本后，用工具栏「脚本」菜单调用已启用的脚本。"
      />

      <div className="flex items-center justify-between mb-2">
        <Text strong>已保存脚本 · {scripts.length}</Text>
        <Button type="primary" size="small" icon={<PlusOutlined />} onClick={openCreate}>
          新建脚本
        </Button>
      </div>

      {scripts.length === 0 ? (
        <Empty
          image={Empty.PRESENTED_IMAGE_SIMPLE}
          description="还没有脚本。点「新建脚本」，用内置示例快速上手 Rhai 文本转换"
        />
      ) : (
        <Table<Script>
          size="small"
          rowKey="id"
          loading={loading}
          dataSource={scripts}
          pagination={false}
          columns={[
            {
              title: "启用",
              dataIndex: "enabled",
              width: 70,
              render: (_, s) => (
                <Switch
                  size="small"
                  checked={s.enabled}
                  onChange={(v) => void handleToggle(s, v)}
                />
              ),
            },
            { title: "名称", dataIndex: "name" },
            {
              title: "说明",
              dataIndex: "description",
              ellipsis: true,
              render: (t: string) => (
                <Text type="secondary" style={{ fontSize: 12 }}>
                  {t || "—"}
                </Text>
              ),
            },
            {
              title: "操作",
              width: 120,
              render: (_, s) => (
                <Space>
                  <Button
                    size="small"
                    type="text"
                    icon={<Pencil size={14} />}
                    onClick={() => openEdit(s)}
                  />
                  <Popconfirm
                    title="删除该脚本？"
                    onConfirm={() => void handleDelete(s.id)}
                    okText="删除"
                    okButtonProps={{ danger: true }}
                  >
                    <Button size="small" type="text" danger icon={<Trash2 size={14} />} />
                  </Popconfirm>
                </Space>
              ),
            },
          ]}
        />
      )}

      <Modal
        title={editingId === null ? "新建脚本" : "编辑脚本"}
        open={modalOpen}
        onCancel={() => setModalOpen(false)}
        onOk={() => void handleSave()}
        okText="保存"
        width={720}
        destroyOnClose
      >
        <Form form={form} layout="vertical">
          <Form.Item
            name="name"
            label="名称"
            rules={[{ required: true, message: "请输入脚本名" }]}
          >
            <Input placeholder="如：规范化标题 / 中英文加空格" />
          </Form.Item>
          <Form.Item name="description" label="说明">
            <Input placeholder="选填：这个脚本做什么" />
          </Form.Item>
          <Form.Item name="trigger" label="作用范围" hidden initialValue="selection">
            <Input />
          </Form.Item>
          <Form.Item
            name="code"
            label="Rhai 代码（变量 input = 选中文本，末表达式 = 输出）"
            rules={[{ required: true, message: "请输入脚本代码" }]}
          >
            <Input.TextArea
              rows={10}
              spellCheck={false}
              style={{ fontFamily: "var(--kb-mono, monospace)", fontSize: 13 }}
            />
          </Form.Item>
          <Form.Item name="enabled" label="启用" valuePropName="checked" initialValue={true}>
            <Switch />
          </Form.Item>

          {/* 试运行 */}
          <div
            style={{
              borderTop: "1px solid var(--ant-color-border-secondary, #f0f0f0)",
              paddingTop: 12,
            }}
          >
            <Text strong>试运行</Text>
            <Input.TextArea
              className="mt-2"
              rows={2}
              value={testInput}
              onChange={(e) => setTestInput(e.target.value)}
              placeholder="试运行的输入文本"
            />
            <Space className="mt-2">
              <Button
                icon={<PlayCircleOutlined />}
                loading={testing}
                onClick={() => void handleTest()}
              >
                试运行
              </Button>
            </Space>
            {testOutput !== null && (
              <Paragraph
                className="mt-2"
                style={{
                  background: "var(--ant-color-fill-quaternary, #fafafa)",
                  padding: 8,
                  borderRadius: 6,
                  whiteSpace: "pre-wrap",
                  fontFamily: "var(--kb-mono, monospace)",
                  fontSize: 12,
                  marginBottom: 0,
                }}
              >
                {testOutput || "（空结果）"}
              </Paragraph>
            )}
          </div>
        </Form>
      </Modal>
    </Card>
  );
}
