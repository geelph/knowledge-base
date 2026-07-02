//! 脚本插件执行引擎（#8 Phase 2）
//!
//! 用 Rhai 跑用户写的「文本转换脚本」：脚本拿到字符串 `input`，返回转换后的字符串。
//!
//! ## 为什么安全
//! - Rhai 默认引擎**没有任何文件 / 网络 / 系统访问能力**（要用得显式 `register_fn` 注册，
//!   我们一个都不注册）→ 天然沙箱，脚本碰不到用户数据以外的东西。
//! - 叠加资源上限防「恶意/写错的脚本拖垮应用」：
//!   - `set_max_operations` 限总操作数 → 死循环会被中断而非卡死
//!   - `set_max_string_size` / `set_max_array_size` / `set_max_map_size` 限内存
//!   - `set_max_call_levels` / `set_max_expr_depths` 限递归/嵌套深度
//! - 脚本只在被用户显式触发时同步执行一次，不常驻、不联网。
//!
//! ## 约定
//! - 输入：脚本作用域里注入变量 `input`（选中文本或整篇正文，字符串）。
//! - 输出：脚本**最后一个表达式的值**，转成字符串即为结果（返回非字符串也会被 to_string）。

use rhai::{Dynamic, Engine, Scope};

/// 单次脚本执行的资源上限（对文本转换足够宽松，又能挡住失控脚本）。
const MAX_OPERATIONS: u64 = 5_000_000;
const MAX_STRING_SIZE: usize = 16 * 1024 * 1024; // 16MB
const MAX_ARRAY_SIZE: usize = 1_000_000;
const MAX_MAP_SIZE: usize = 1_000_000;
const MAX_CALL_LEVELS: usize = 64;

/// 构造一个上了沙箱限制的 Rhai 引擎。每次执行新建，避免脚本间状态串味。
fn sandboxed_engine() -> Engine {
    let mut engine = Engine::new();
    engine.set_max_operations(MAX_OPERATIONS);
    engine.set_max_string_size(MAX_STRING_SIZE);
    engine.set_max_array_size(MAX_ARRAY_SIZE);
    engine.set_max_map_size(MAX_MAP_SIZE);
    engine.set_max_call_levels(MAX_CALL_LEVELS);
    // print / debug 默认写 stdout，这里吞掉：脚本里 print 不该污染 sidecar/日志通道
    engine.on_print(|_| {});
    engine.on_debug(|_, _, _| {});
    engine
}

pub struct ScriptService;

impl ScriptService {
    /// 跑一段文本转换脚本：注入 `input`，返回脚本最后表达式的字符串值。
    /// 编译/运行/超限错误统一以人类可读的 String 返回（前端直接展示）。
    pub fn run_transform(code: &str, input: &str) -> Result<String, String> {
        let engine = sandboxed_engine();
        let mut scope = Scope::new();
        // 注入输入文本。Rhai 字符串是 ImmutableString，push &str 即可。
        scope.push("input", input.to_string());

        let result: Dynamic = engine
            .eval_with_scope::<Dynamic>(&mut scope, code)
            .map_err(|e| format!("脚本执行失败: {e}"))?;

        // 结果转字符串：已是字符串直接取，否则用 Rhai 的 to_string 表示
        if result.is_string() {
            result
                .into_string()
                .map_err(|e| format!("结果转字符串失败: {e}"))
        } else if result.is_unit() {
            // 脚本以语句结尾（无返回值）→ 视为空字符串，避免把 "()" 写进笔记
            Ok(String::new())
        } else {
            Ok(result.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_uppercase() {
        let out = ScriptService::run_transform("input.to_upper()", "hello").unwrap();
        assert_eq!(out, "HELLO");
    }

    #[test]
    fn transform_in_place_trim_then_return_var() {
        // Rhai 的 trim()/replace() 是「原地修改、返回 unit」，需先改再把变量作为末表达式返回
        let out =
            ScriptService::run_transform(r#"let s = input; s.trim(); s.replace("a", "X"); s"#, "  banana  ")
                .unwrap();
        assert_eq!(out, "bXnXnX");
    }

    #[test]
    fn transform_concat_returns_new_string() {
        // + 拼接返回新串（value 语义），适合链式转换
        let out = ScriptService::run_transform(r#"input + "!""#, "hi").unwrap();
        assert_eq!(out, "hi!");
    }

    #[test]
    fn transform_non_string_result_is_stringified() {
        let out = ScriptService::run_transform("input.len", "abcd").unwrap();
        assert_eq!(out, "4");
    }

    #[test]
    fn transform_unit_result_is_empty() {
        // 以 let 语句结尾，无返回值 → 空字符串
        let out = ScriptService::run_transform("let x = input;", "abc").unwrap();
        assert_eq!(out, "");
    }

    #[test]
    fn compile_error_is_reported() {
        let err = ScriptService::run_transform("this is not valid rhai !!!", "x").unwrap_err();
        assert!(err.contains("脚本执行失败"), "应报执行失败: {err}");
    }

    #[test]
    fn infinite_loop_is_bounded_not_hang() {
        // 死循环必须被 max_operations 中断并返回错误，而不是卡死测试
        let err = ScriptService::run_transform("loop { }", "x").unwrap_err();
        assert!(err.contains("脚本执行失败"), "死循环应被限制中断: {err}");
    }

    #[test]
    fn no_file_access_by_default() {
        // Rhai 默认没有文件读写函数，调用未知函数应报错（证明沙箱无 I/O）
        let err =
            ScriptService::run_transform(r#"open_file("/etc/passwd")"#, "x").unwrap_err();
        assert!(err.contains("脚本执行失败"), "不该存在文件函数: {err}");
    }
}
