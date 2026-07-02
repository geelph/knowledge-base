//! 脚本插件 DAO（#8 Phase 2）
//!
//! 表结构见 schema.rs::migrate_v47_to_v48。

use rusqlite::{params, Row};

use super::Database;
use crate::error::AppError;
use crate::models::{Script, ScriptInput};

const SELECT_COLUMNS: &str =
    "id, name, description, kind, trigger, code, enabled, created_at, updated_at";

fn row_to_script(row: &Row<'_>) -> rusqlite::Result<Script> {
    Ok(Script {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        kind: row.get(3)?,
        trigger: row.get(4)?,
        code: row.get(5)?,
        enabled: row.get::<_, i32>(6)? != 0,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

impl Database {
    pub fn list_scripts(&self) -> Result<Vec<Script>, AppError> {
        let conn = self.conn_lock()?;
        let sql = format!("SELECT {SELECT_COLUMNS} FROM scripts ORDER BY name");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map([], row_to_script)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn get_script(&self, id: i64) -> Result<Option<Script>, AppError> {
        let conn = self.conn_lock()?;
        let sql = format!("SELECT {SELECT_COLUMNS} FROM scripts WHERE id = ?1");
        let mut stmt = conn.prepare(&sql)?;
        let r = stmt.query_row(params![id], row_to_script).ok();
        Ok(r)
    }

    pub fn create_script(&self, input: &ScriptInput) -> Result<Script, AppError> {
        let conn = self.conn_lock()?;
        conn.execute(
            "INSERT INTO scripts (name, description, kind, trigger, code, enabled)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                input.name,
                input.description,
                input.kind,
                input.trigger,
                input.code,
                input.enabled as i32,
            ],
        )?;
        let id = conn.last_insert_rowid();
        let sql = format!("SELECT {SELECT_COLUMNS} FROM scripts WHERE id = ?1");
        let mut stmt = conn.prepare(&sql)?;
        let r = stmt.query_row(params![id], row_to_script)?;
        Ok(r)
    }

    pub fn update_script(&self, id: i64, input: &ScriptInput) -> Result<Script, AppError> {
        let conn = self.conn_lock()?;
        let affected = conn.execute(
            "UPDATE scripts
             SET name = ?1, description = ?2, kind = ?3, trigger = ?4, code = ?5,
                 enabled = ?6, updated_at = datetime('now', 'localtime')
             WHERE id = ?7",
            params![
                input.name,
                input.description,
                input.kind,
                input.trigger,
                input.code,
                input.enabled as i32,
                id,
            ],
        )?;
        if affected == 0 {
            return Err(AppError::NotFound(format!("script {id} 不存在")));
        }
        let sql = format!("SELECT {SELECT_COLUMNS} FROM scripts WHERE id = ?1");
        let mut stmt = conn.prepare(&sql)?;
        let r = stmt.query_row(params![id], row_to_script)?;
        Ok(r)
    }

    pub fn delete_script(&self, id: i64) -> Result<bool, AppError> {
        let conn = self.conn_lock()?;
        let affected = conn.execute("DELETE FROM scripts WHERE id = ?1", params![id])?;
        Ok(affected > 0)
    }

    pub fn set_script_enabled(&self, id: i64, enabled: bool) -> Result<(), AppError> {
        let conn = self.conn_lock()?;
        let affected = conn.execute(
            "UPDATE scripts SET enabled = ?1, updated_at = datetime('now', 'localtime')
             WHERE id = ?2",
            params![enabled as i32, id],
        )?;
        if affected == 0 {
            return Err(AppError::NotFound(format!("script {id} 不存在")));
        }
        Ok(())
    }
}
