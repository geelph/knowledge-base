//! 定时推送服务层
//!
//! 子模块：
//! - `schedule_calc`：算"下次运行时刻"
//! - `dispatch`：把生成内容投递到各通道（MVP 系统通知）
//! - `scheduler`：常驻调度 loop（事件驱动 + 兜底唤醒，骨架照搬 task_reminder）
//!
//! 业务编排（跑提示词 → 投递 → 记日志）集中在本文件的 `PushService`。

pub mod dispatch;
pub mod schedule_calc;
pub mod scheduler;

use chrono::Local;
use tauri::AppHandle;

use crate::database::Database;
use crate::models::PushJob;
use crate::services::ai::AiService;

pub struct PushService;

impl PushService {
    /// 以"现在"为基准算下次运行时刻字符串（命令层建/改/启用推送时用）。
    pub fn next_run_from_now(
        schedule_time: &str,
        repeat_kind: &str,
        repeat_weekdays: Option<&str>,
    ) -> Option<String> {
        schedule_calc::compute_next_run(
            schedule_time,
            repeat_kind,
            repeat_weekdays,
            Local::now().naive_local(),
        )
    }

    /// 执行一条推送：跑提示词 → 投递 → 写运行日志。
    ///
    /// 无人值守：内部消化所有错误（写 run_log + 失败通知），绝不 panic、绝不向上抛，
    /// 以免拖垮调度 loop。MVP 只处理生成型（source_kind=none）；阶段 2 在调 AI 前按
    /// source_kind 抓数据拼进 prompt。
    pub async fn run_job(app: &AppHandle, db: &Database, job: &PushJob) {
        let run_at = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let prompt = job.prompt.clone();

        match AiService::complete_once(db, &prompt, job.model_id).await {
            Ok(content) => {
                dispatch::dispatch(app, job, &content);
                if let Err(e) =
                    db.insert_push_run_log(job.id, &run_at, "success", 1, Some(&content), None)
                {
                    log::warn!("[push] 写运行日志失败 (job #{}): {}", job.id, e);
                }
                log::info!("[push] 推送 #{} 「{}」执行成功", job.id, job.name);
            }
            Err(e) => {
                let err = e.to_string();
                let _ = db.insert_push_run_log(job.id, &run_at, "failed", 0, None, Some(&err));
                dispatch::dispatch_failure(app, job, &err);
                log::warn!("[push] 推送 #{} 「{}」执行失败: {}", job.id, job.name, err);
            }
        }
    }
}
