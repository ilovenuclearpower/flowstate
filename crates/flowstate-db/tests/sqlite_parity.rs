// Integration tests that exercise every Database trait method against the
// in-memory SQLite backend.  The actual test logic lives in `common/mod.rs`
// so that the same assertions can be re-used for Postgres.

mod common;

use std::sync::Arc;
use flowstate_db::Database;

async fn make_db() -> Arc<dyn Database> {
    Arc::new(flowstate_db::SqliteDatabase::open_in_memory().unwrap())
}

#[tokio::test]
async fn project_crud() {
    let db = make_db().await;
    common::test_project_crud(&*db).await;
}

#[tokio::test]
async fn task_crud() {
    let db = make_db().await;
    common::test_task_crud(&*db).await;
}

#[tokio::test]
async fn task_filtering() {
    let db = make_db().await;
    common::test_task_filtering(&*db).await;
}

#[tokio::test]
async fn task_sort_order() {
    let db = make_db().await;
    common::test_task_sort_order(&*db).await;
}

#[tokio::test]
async fn count_by_status() {
    let db = make_db().await;
    common::test_count_by_status(&*db).await;
}

#[tokio::test]
async fn child_tasks() {
    let db = make_db().await;
    common::test_child_tasks(&*db).await;
}

#[tokio::test]
async fn claude_run_lifecycle() {
    let db = make_db().await;
    common::test_claude_run_lifecycle(&*db).await;
}

#[tokio::test]
async fn claim_empty() {
    let db = make_db().await;
    common::test_claim_empty(&*db).await;
}

#[tokio::test]
async fn stale_runs() {
    let db = make_db().await;
    common::test_stale_runs(&*db).await;
}

#[tokio::test]
async fn task_links() {
    let db = make_db().await;
    common::test_task_links(&*db).await;
}

#[tokio::test]
async fn task_prs() {
    let db = make_db().await;
    common::test_task_prs(&*db).await;
}

#[tokio::test]
async fn attachments() {
    let db = make_db().await;
    common::test_attachments(&*db).await;
}

#[tokio::test]
async fn api_keys() {
    let db = make_db().await;
    common::test_api_keys(&*db).await;
}

#[tokio::test]
async fn sprint_crud() {
    let db = make_db().await;
    common::test_sprint_crud(&*db).await;
}

#[tokio::test]
async fn subtask_workflow() {
    let db = make_db().await;
    common::test_subtask_workflow(&*db).await;
}
