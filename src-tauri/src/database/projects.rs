use rusqlite::params;

use crate::error::AppError;
use crate::models::{CreateProjectInput, Project, UpdateProjectInput};

impl super::Database {
    /// 列出所有项目（带任务计数）；可按 archived 过滤。
    ///
    /// - `include_archived=false`：只列出未归档（首页默认行为）
    /// - `include_archived=true`：含已归档（设置/归档管理页用）
    pub fn list_projects(&self, include_archived: bool) -> Result<Vec<Project>, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;
        let where_sql = if include_archived {
            ""
        } else {
            "WHERE p.archived = 0"
        };
        let sql = format!(
            "SELECT p.id, p.name, p.description, p.color, p.start_date, p.end_date,
                    p.archived, p.sort_order, p.created_at, p.updated_at,
                    COALESCE(SUM(CASE WHEN t.status = 0 THEN 1 ELSE 0 END), 0) AS active_cnt,
                    COALESCE(SUM(CASE WHEN t.status = 1 THEN 1 ELSE 0 END), 0) AS done_cnt
             FROM projects p
             LEFT JOIN tasks t ON t.project_id = p.id AND t.parent_task_id IS NULL
             {}
             GROUP BY p.id
             ORDER BY p.archived ASC, p.sort_order ASC, p.id ASC",
            where_sql,
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(Project {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    color: row.get(3)?,
                    start_date: row.get(4)?,
                    end_date: row.get(5)?,
                    archived: row.get::<_, i32>(6)? != 0,
                    sort_order: row.get(7)?,
                    created_at: row.get(8)?,
                    updated_at: row.get(9)?,
                    active_task_count: row.get(10)?,
                    done_task_count: row.get(11)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 获取单个项目（含任务计数）
    pub fn get_project(&self, id: i64) -> Result<Option<Project>, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;
        let project = conn
            .query_row(
                "SELECT p.id, p.name, p.description, p.color, p.start_date, p.end_date,
                        p.archived, p.sort_order, p.created_at, p.updated_at,
                        COALESCE((SELECT COUNT(*) FROM tasks
                                  WHERE project_id = p.id AND parent_task_id IS NULL AND status = 0), 0),
                        COALESCE((SELECT COUNT(*) FROM tasks
                                  WHERE project_id = p.id AND parent_task_id IS NULL AND status = 1), 0)
                 FROM projects p WHERE p.id = ?1",
                params![id],
                |row| {
                    Ok(Project {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        description: row.get(2)?,
                        color: row.get(3)?,
                        start_date: row.get(4)?,
                        end_date: row.get(5)?,
                        archived: row.get::<_, i32>(6)? != 0,
                        sort_order: row.get(7)?,
                        created_at: row.get(8)?,
                        updated_at: row.get(9)?,
                        active_task_count: row.get(10)?,
                        done_task_count: row.get(11)?,
                    })
                },
            )
            .ok();
        Ok(project)
    }

    /// 创建项目；name 不强制唯一（用户允许同名项目，比如不同年份）
    pub fn create_project(&self, input: CreateProjectInput) -> Result<i64, AppError> {
        let name = input.name.trim();
        if name.is_empty() {
            return Err(AppError::InvalidInput("项目名称不能为空".into()));
        }
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;
        let color = input
            .color
            .as_deref()
            .filter(|c| !c.trim().is_empty())
            .unwrap_or("#1677ff")
            .to_string();
        conn.execute(
            "INSERT INTO projects (name, description, color, start_date, end_date)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                name,
                input.description,
                color,
                input.start_date,
                input.end_date,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// 更新项目（动态 SET；只改传入的字段）
    pub fn update_project(
        &self,
        id: i64,
        input: UpdateProjectInput,
    ) -> Result<bool, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;
        let mut sets: Vec<&'static str> = Vec::new();
        let mut binds: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(name) = input.name {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                return Err(AppError::InvalidInput("项目名称不能为空".into()));
            }
            sets.push("name = ?");
            binds.push(Box::new(trimmed.to_string()));
        }
        if input.clear_description.unwrap_or(false) {
            sets.push("description = NULL");
        } else if let Some(d) = input.description {
            sets.push("description = ?");
            binds.push(Box::new(d));
        }
        if let Some(c) = input.color {
            sets.push("color = ?");
            binds.push(Box::new(c));
        }
        if input.clear_start_date.unwrap_or(false) {
            sets.push("start_date = NULL");
        } else if let Some(s) = input.start_date {
            sets.push("start_date = ?");
            binds.push(Box::new(s));
        }
        if input.clear_end_date.unwrap_or(false) {
            sets.push("end_date = NULL");
        } else if let Some(e) = input.end_date {
            sets.push("end_date = ?");
            binds.push(Box::new(e));
        }
        if let Some(a) = input.archived {
            sets.push("archived = ?");
            binds.push(Box::new(if a { 1i32 } else { 0i32 }));
        }
        if let Some(s) = input.sort_order {
            sets.push("sort_order = ?");
            binds.push(Box::new(s));
        }

        if sets.is_empty() {
            return Ok(false);
        }
        sets.push("updated_at = datetime('now','localtime')");
        let sql = format!("UPDATE projects SET {} WHERE id = ?", sets.join(", "));
        binds.push(Box::new(id));
        let affected = conn.execute(
            &sql,
            rusqlite::params_from_iter(binds.iter().map(|b| b.as_ref())),
        )?;
        Ok(affected > 0)
    }

    /// 删除项目。tasks.project_id 因 ON DELETE SET NULL 自动落到"无项目"。
    pub fn delete_project(&self, id: i64) -> Result<bool, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;
        let affected = conn.execute("DELETE FROM projects WHERE id = ?1", params![id])?;
        Ok(affected > 0)
    }
}

#[cfg(test)]
mod tests {
    use crate::database::Database;
    use crate::models::{CreateProjectInput, UpdateProjectInput};

    fn fresh() -> Database {
        Database::init(":memory:").unwrap()
    }

    #[test]
    fn create_and_get_project() {
        let db = fresh();
        let id = db
            .create_project(CreateProjectInput {
                name: "v1.11 发版".into(),
                description: Some("Sprint 2 集中冲刺".into()),
                color: None,
                start_date: Some("2026-05-01".into()),
                end_date: Some("2026-05-31".into()),
            })
            .unwrap();
        let p = db.get_project(id).unwrap().expect("项目应存在");
        assert_eq!(p.name, "v1.11 发版");
        assert_eq!(p.color, "#1677ff"); // 默认色
        assert_eq!(p.start_date.as_deref(), Some("2026-05-01"));
        assert!(!p.archived);
        assert_eq!(p.active_task_count, 0);
    }

    #[test]
    fn update_project_partial() {
        let db = fresh();
        let id = db
            .create_project(CreateProjectInput {
                name: "X".into(),
                description: None,
                color: None,
                start_date: None,
                end_date: None,
            })
            .unwrap();
        // 只改 color，其他不动
        db.update_project(
            id,
            UpdateProjectInput {
                color: Some("#ff0000".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let p = db.get_project(id).unwrap().unwrap();
        assert_eq!(p.color, "#ff0000");
        assert_eq!(p.name, "X");
    }

    #[test]
    fn reject_empty_name() {
        let db = fresh();
        let err = db
            .create_project(CreateProjectInput {
                name: "  ".into(),
                description: None,
                color: None,
                start_date: None,
                end_date: None,
            })
            .unwrap_err();
        assert!(err.to_string().contains("不能为空"));
    }

    #[test]
    fn list_projects_filter_archived() {
        let db = fresh();
        let active_id = db
            .create_project(CreateProjectInput {
                name: "A".into(),
                description: None,
                color: None,
                start_date: None,
                end_date: None,
            })
            .unwrap();
        let archived_id = db
            .create_project(CreateProjectInput {
                name: "B".into(),
                description: None,
                color: None,
                start_date: None,
                end_date: None,
            })
            .unwrap();
        db.update_project(
            archived_id,
            UpdateProjectInput {
                archived: Some(true),
                ..Default::default()
            },
        )
        .unwrap();

        let active_only = db.list_projects(false).unwrap();
        assert_eq!(active_only.len(), 1);
        assert_eq!(active_only[0].id, active_id);

        let all = db.list_projects(true).unwrap();
        assert_eq!(all.len(), 2);
    }
}
