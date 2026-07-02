//! 脚本插件 Commands（#8 Phase 2）
//!
//! CRUD + 执行。执行分两种：
//! - `script_run_preview`：跑任意 code + input（脚本编辑器的「试运行」用）
//! - `script_run`：按 id 跑已保存脚本（编辑器里对选中文本应用转换用）

use crate::models::{Script, ScriptInput};
use crate::services::script::ScriptService;
use crate::state::AppState;

#[tauri::command]
pub fn script_list(state: tauri::State<'_, AppState>) -> Result<Vec<Script>, String> {
    state.db.list_scripts().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn script_create(
    state: tauri::State<'_, AppState>,
    input: ScriptInput,
) -> Result<Script, String> {
    state.db.create_script(&input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn script_update(
    state: tauri::State<'_, AppState>,
    id: i64,
    input: ScriptInput,
) -> Result<Script, String> {
    state.db.update_script(id, &input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn script_delete(state: tauri::State<'_, AppState>, id: i64) -> Result<bool, String> {
    state.db.delete_script(id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn script_set_enabled(
    state: tauri::State<'_, AppState>,
    id: i64,
    enabled: bool,
) -> Result<(), String> {
    state
        .db
        .set_script_enabled(id, enabled)
        .map_err(|e| e.to_string())
}

/// 试运行任意脚本（编辑器「试运行」按钮）：直接跑 code + input，返回输出。
/// 沙箱执行是 CPU 密集但有 max_operations 上限，用同步 command 即可。
#[tauri::command]
pub fn script_run_preview(code: String, input: String) -> Result<String, String> {
    ScriptService::run_transform(&code, &input)
}

/// 按 id 跑已保存脚本，对 input 做转换后返回结果（编辑器对选中文本应用转换）。
#[tauri::command]
pub fn script_run(
    state: tauri::State<'_, AppState>,
    id: i64,
    input: String,
) -> Result<String, String> {
    let script = state
        .db
        .get_script(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("脚本 {id} 不存在"))?;
    ScriptService::run_transform(&script.code, &input)
}
