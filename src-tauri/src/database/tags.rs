use rusqlite::{params, OptionalExtension};

use crate::error::AppError;
use crate::models::{Note, Tag};

use super::Database;

impl Database {
    // ─── 标签 DAO ─────────────────────────────────

    /// 创建标签
    pub fn create_tag(&self, name: &str, color: Option<&str>) -> Result<Tag, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;

        conn.execute(
            "INSERT INTO tags (name, color) VALUES (?1, ?2)",
            params![name, color],
        )?;

        let id = conn.last_insert_rowid();

        Ok(Tag {
            id,
            name: name.to_string(),
            color: color.map(|c| c.to_string()),
            note_count: 0,
        })
    }

    /// 获取所有标签（带笔记计数，按笔记数降序）
    pub fn list_tags(&self) -> Result<Vec<Tag>, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;

        let mut stmt = conn.prepare(
            "SELECT t.id, t.name, t.color, COUNT(nt.note_id) as note_count
             FROM tags t
             LEFT JOIN note_tags nt ON t.id = nt.tag_id
             LEFT JOIN notes n ON nt.note_id = n.id AND n.is_deleted = 0
             GROUP BY t.id
             ORDER BY note_count DESC, t.name",
        )?;

        let tags = stmt
            .query_map([], |row| {
                Ok(Tag {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    color: row.get(2)?,
                    note_count: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(tags)
    }

    /// 修改标签颜色（传 None 清空颜色走默认样式）
    pub fn set_tag_color(&self, id: i64, color: Option<&str>) -> Result<(), AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;

        let affected = conn.execute(
            "UPDATE tags SET color = ?1 WHERE id = ?2",
            params![color, id],
        )?;

        if affected == 0 {
            return Err(AppError::NotFound(format!("标签 {} 不存在", id)));
        }

        Ok(())
    }

    /// 重命名标签
    pub fn rename_tag(&self, id: i64, name: &str) -> Result<(), AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;

        let affected =
            conn.execute("UPDATE tags SET name = ?1 WHERE id = ?2", params![name, id])?;

        if affected == 0 {
            return Err(AppError::NotFound(format!("标签 {} 不存在", id)));
        }

        Ok(())
    }

    /// 删除标签（同时删除关联）
    pub fn delete_tag(&self, id: i64) -> Result<bool, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;

        // 先删除关联关系
        conn.execute("DELETE FROM note_tags WHERE tag_id = ?1", params![id])?;

        // 再删除标签本身
        let affected = conn.execute("DELETE FROM tags WHERE id = ?1", params![id])?;

        Ok(affected > 0)
    }

    /// 按名字获取标签 id；不存在则创建。导入流程使用。
    ///
    /// 名字会做 trim；空名字直接报错而不是默默忽略。
    pub fn get_or_create_tag_by_name(&self, name: &str) -> Result<i64, AppError> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(AppError::InvalidInput("标签名不能为空".into()));
        }
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;
        // 先查
        if let Ok(id) = conn.query_row(
            "SELECT id FROM tags WHERE name = ?1",
            params![trimmed],
            |row| row.get::<_, i64>(0),
        ) {
            return Ok(id);
        }
        // 再建
        conn.execute(
            "INSERT INTO tags (name, color) VALUES (?1, NULL)",
            params![trimmed],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Bug 12a 同步 V1 用：把笔记的标签关联**整体替换**成给定 name 列表（按 name 跨端）。
    ///
    /// - 空白 / 重复名字自动去掉（先 trim、再去重）
    /// - 按 name find-or-create 本地 tag id（颜色不动 — color 是本地偏好不该被远端覆盖）
    /// - 用事务一次性 DELETE 旧关联 + INSERT 新关联
    /// - **不动 `notes.updated_at`**（标签变更是元数据，不是内容变更，不该触发 sync diff）
    pub fn sync_note_tags(&self, note_id: i64, tag_names: &[String]) -> Result<(), AppError> {
        // 规范化 name 列表：trim + 去掉空 + 按字符串去重（保持稳定顺序）
        let mut seen = std::collections::HashSet::new();
        let normalized: Vec<&str> = tag_names
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty() && seen.insert(s.to_string()))
            .collect();

        let mut conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;
        let tx = conn.transaction()?;

        // 1) 清掉本笔记现有关联（不删 tag 本身 — 别的笔记可能还在用）
        tx.execute("DELETE FROM note_tags WHERE note_id = ?1", params![note_id])?;

        // 2) 按 name find-or-create + 新增关联
        for name in normalized {
            // 先查
            let tag_id: i64 = match tx
                .query_row(
                    "SELECT id FROM tags WHERE name = ?1",
                    params![name],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?
            {
                Some(id) => id,
                None => {
                    tx.execute(
                        "INSERT INTO tags (name, color) VALUES (?1, NULL)",
                        params![name],
                    )?;
                    tx.last_insert_rowid()
                }
            };
            tx.execute(
                "INSERT OR IGNORE INTO note_tags (note_id, tag_id) VALUES (?1, ?2)",
                params![note_id, tag_id],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    /// Bug 12a 同步 V1 用：一次性拿全库 (note_id → [tag_name, ...]) 映射，给 compute_local_manifest
    /// 填 ManifestEntry.tags 用。比"每条 entry 单独查 get_note_tags(id)"快很多。
    pub fn list_all_note_tag_names(
        &self,
    ) -> Result<std::collections::HashMap<i64, Vec<String>>, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT nt.note_id, t.name
             FROM note_tags nt
             JOIN tags t ON t.id = nt.tag_id
             ORDER BY nt.note_id, t.name",
        )?;
        let rows: Vec<(i64, String)> = stmt
            .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?
            .filter_map(|r| r.ok())
            .collect();

        let mut out: std::collections::HashMap<i64, Vec<String>> =
            std::collections::HashMap::new();
        for (note_id, name) in rows {
            out.entry(note_id).or_default().push(name);
        }
        Ok(out)
    }

    /// 给笔记添加标签
    pub fn add_tag_to_note(&self, note_id: i64, tag_id: i64) -> Result<(), AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;

        conn.execute(
            "INSERT OR IGNORE INTO note_tags (note_id, tag_id) VALUES (?1, ?2)",
            params![note_id, tag_id],
        )?;

        Ok(())
    }

    /// 批量关联：给多篇笔记 × 多个标签 一次性打上关联；返回新增的关联条数
    ///
    /// - 使用 `INSERT OR IGNORE` 自然去重：已存在的 (note_id, tag_id) 对不重复插入
    /// - 事务内一次性 batch，避免多次 IPC / 多次锁
    pub fn add_tags_to_notes_batch(
        &self,
        note_ids: &[i64],
        tag_ids: &[i64],
    ) -> Result<usize, AppError> {
        if note_ids.is_empty() || tag_ids.is_empty() {
            return Ok(0);
        }
        let mut conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;
        let tx = conn.transaction()?;
        let mut inserted = 0usize;
        {
            let mut stmt =
                tx.prepare("INSERT OR IGNORE INTO note_tags (note_id, tag_id) VALUES (?1, ?2)")?;
            for nid in note_ids {
                for tid in tag_ids {
                    inserted += stmt.execute(params![nid, tid])?;
                }
            }
        }
        tx.commit()?;
        Ok(inserted)
    }

    /// 移除笔记的标签
    pub fn remove_tag_from_note(&self, note_id: i64, tag_id: i64) -> Result<bool, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;

        let affected = conn.execute(
            "DELETE FROM note_tags WHERE note_id = ?1 AND tag_id = ?2",
            params![note_id, tag_id],
        )?;

        Ok(affected > 0)
    }

    /// 获取笔记的所有标签
    pub fn get_note_tags(&self, note_id: i64) -> Result<Vec<Tag>, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;

        let mut stmt = conn.prepare(
            "SELECT t.id, t.name, t.color, COUNT(nt2.note_id) as note_count
             FROM tags t
             INNER JOIN note_tags nt ON t.id = nt.tag_id AND nt.note_id = ?1
             LEFT JOIN note_tags nt2 ON t.id = nt2.tag_id
             LEFT JOIN notes n ON nt2.note_id = n.id AND n.is_deleted = 0
             GROUP BY t.id
             ORDER BY t.name",
        )?;

        let tags = stmt
            .query_map(params![note_id], |row| {
                Ok(Tag {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    color: row.get(2)?,
                    note_count: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(tags)
    }

    /// 获取标签下的笔记列表（分页）
    pub fn list_notes_by_tag(
        &self,
        tag_id: i64,
        page: usize,
        page_size: usize,
    ) -> Result<(Vec<Note>, usize), AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;

        // 查询总数（T-003: 排除隐藏笔记）
        let total: usize = conn.query_row(
            "SELECT COUNT(*) FROM note_tags nt
             INNER JOIN notes n ON nt.note_id = n.id AND n.is_deleted = 0 AND n.is_hidden = 0
             WHERE nt.tag_id = ?1",
            params![tag_id],
            |row| row.get(0),
        )?;

        // 查询分页数据
        let offset = (page.saturating_sub(1)) * page_size;

        let mut stmt = conn.prepare(
            "SELECT n.id, n.title, n.content, n.folder_id, n.is_daily, n.daily_date,
                    n.is_pinned, n.is_hidden, n.is_encrypted, n.word_count, n.created_at, n.updated_at, n.source_file_path, n.source_file_type, n.sort_order
             FROM notes n
             INNER JOIN note_tags nt ON n.id = nt.note_id
             WHERE nt.tag_id = ?1 AND n.is_deleted = 0 AND n.is_hidden = 0
             ORDER BY n.updated_at DESC
             LIMIT ?2 OFFSET ?3",
        )?;

        let notes = stmt
            .query_map(params![tag_id, page_size as i64, offset as i64], |row| {
                Ok(Note {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    content: row.get(2)?,
                    folder_id: row.get(3)?,
                    is_daily: row.get::<_, i32>(4)? != 0,
                    daily_date: row.get(5)?,
                    is_pinned: row.get::<_, i32>(6)? != 0,
                    is_hidden: row.get::<_, i32>(7)? != 0,
                    is_encrypted: row.get::<_, i32>(8)? != 0,
                    word_count: row.get(9)?,
                    created_at: row.get(10)?,
                    updated_at: row.get(11)?,
                    source_file_path: row.get(12)?,
                    source_file_type: row.get(13)?,
                    sort_order: row.get(14)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok((notes, total))
    }
}

#[cfg(test)]
mod sync_tag_tests {
    //! Bug 12a：sync_note_tags / list_all_note_tag_names

    use crate::database::Database;
    use crate::models::NoteInput;

    fn fresh() -> Database {
        Database::init(":memory:").unwrap()
    }

    fn note_tag_names(db: &Database, note_id: i64) -> Vec<String> {
        let mut names: Vec<String> =
            db.get_note_tags(note_id).unwrap().into_iter().map(|t| t.name).collect();
        names.sort();
        names
    }

    #[test]
    fn sync_note_tags_replace_set() {
        let db = fresh();
        let n = db
            .create_note(&NoteInput {
                title: "x".into(),
                content: "y".into(),
                folder_id: None,
            })
            .unwrap();

        // 初始无标签
        assert!(note_tag_names(&db, n.id).is_empty());

        // 设两个标签（不存在的会自动创建）
        db.sync_note_tags(n.id, &vec!["工作".into(), "周报".into()]).unwrap();
        assert_eq!(note_tag_names(&db, n.id), vec!["周报".to_string(), "工作".to_string()]);

        // 改成新集合（去掉"周报"，加"个人"）
        db.sync_note_tags(n.id, &vec!["工作".into(), "个人".into()]).unwrap();
        assert_eq!(note_tag_names(&db, n.id), vec!["个人".to_string(), "工作".to_string()]);

        // 清空
        db.sync_note_tags(n.id, &[]).unwrap();
        assert!(note_tag_names(&db, n.id).is_empty());
    }

    #[test]
    fn sync_note_tags_normalizes_input() {
        let db = fresh();
        let n = db
            .create_note(&NoteInput {
                title: "x".into(),
                content: "y".into(),
                folder_id: None,
            })
            .unwrap();
        db.sync_note_tags(
            n.id,
            &vec!["  工作  ".into(), "工作".into(), "".into(), "  ".into(), "周报".into()],
        )
        .unwrap();
        // trim + 去重 + 跳过空 → 剩 ["工作","周报"]
        assert_eq!(note_tag_names(&db, n.id), vec!["周报".to_string(), "工作".to_string()]);
    }

    #[test]
    fn sync_note_tags_does_not_delete_other_notes_relations() {
        let db = fresh();
        let n1 = db
            .create_note(&NoteInput {
                title: "a".into(),
                content: "y".into(),
                folder_id: None,
            })
            .unwrap();
        let n2 = db
            .create_note(&NoteInput {
                title: "b".into(),
                content: "y".into(),
                folder_id: None,
            })
            .unwrap();
        db.sync_note_tags(n1.id, &vec!["共享".into()]).unwrap();
        db.sync_note_tags(n2.id, &vec!["共享".into(), "n2only".into()]).unwrap();

        // 改 n1 → 不影响 n2
        db.sync_note_tags(n1.id, &vec![]).unwrap();
        assert!(note_tag_names(&db, n1.id).is_empty());
        assert_eq!(
            note_tag_names(&db, n2.id),
            vec!["n2only".to_string(), "共享".to_string()]
        );
    }

    #[test]
    fn list_all_note_tag_names_groups_by_note() {
        let db = fresh();
        let n1 = db
            .create_note(&NoteInput {
                title: "a".into(),
                content: "y".into(),
                folder_id: None,
            })
            .unwrap();
        let n2 = db
            .create_note(&NoteInput {
                title: "b".into(),
                content: "y".into(),
                folder_id: None,
            })
            .unwrap();
        let n3 = db
            .create_note(&NoteInput {
                title: "c".into(),
                content: "y".into(),
                folder_id: None,
            })
            .unwrap();
        db.sync_note_tags(n1.id, &vec!["x".into(), "y".into()]).unwrap();
        db.sync_note_tags(n2.id, &vec!["x".into()]).unwrap();
        // n3 无 tag

        let map = db.list_all_note_tag_names().unwrap();
        let mut n1_tags = map.get(&n1.id).cloned().unwrap_or_default();
        n1_tags.sort();
        assert_eq!(n1_tags, vec!["x".to_string(), "y".to_string()]);
        assert_eq!(map.get(&n2.id).cloned().unwrap_or_default(), vec!["x".to_string()]);
        assert!(map.get(&n3.id).is_none(), "无标签的 note_id 不该出现在 map 里");
    }
}
