// Integration tests that exercise every Database trait method against a real
// Postgres backend.  Each test is marked `#[ignore]` because it requires a
// running Postgres instance and `DATABASE_URL` to be set.
//
// Run with:
//   DATABASE_URL="postgres://user:pass@localhost/flowstate_test" \
//     cargo test -p flowstate-db --features postgres -- --ignored

#![cfg(feature = "postgres")]

mod common;

use std::sync::Arc;
use flowstate_db::Database;

/// Connect to the test Postgres database and TRUNCATE all tables so each
/// test starts with a clean slate.  The `pool` field on `PostgresDatabase`
/// is `pub(crate)` so integration tests cannot access it directly; we open
/// a second throwaway `sqlx::PgPool` for the cleanup step.
async fn make_db() -> Arc<dyn Database> {
    let url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set for postgres parity tests");

    let db = flowstate_db::postgres::PostgresDatabase::connect(&url)
        .await
        .unwrap();

    // Clean all tables in reverse FK-dependency order.
    let cleanup_pool = sqlx::PgPool::connect(&url).await.unwrap();
    sqlx::query(
        "TRUNCATE
            task_prs,
            attachments,
            task_links,
            claude_runs,
            task_labels,
            task_verifications,
            verification_run_steps,
            verification_runs,
            verification_steps,
            verification_profiles,
            commit_links,
            tasks,
            sprints,
            labels,
            projects,
            api_keys
         CASCADE",
    )
    .execute(&cleanup_pool)
    .await
    .unwrap();
    cleanup_pool.close().await;

    Arc::new(db)
}

#[tokio::test]
#[ignore]
async fn project_crud() {
    let db = make_db().await;
    common::test_project_crud(&*db).await;
}

#[tokio::test]
#[ignore]
async fn task_crud() {
    let db = make_db().await;
    common::test_task_crud(&*db).await;
}

#[tokio::test]
#[ignore]
async fn task_filtering() {
    let db = make_db().await;
    common::test_task_filtering(&*db).await;
}

#[tokio::test]
#[ignore]
async fn task_sort_order() {
    let db = make_db().await;
    common::test_task_sort_order(&*db).await;
}

#[tokio::test]
#[ignore]
async fn count_by_status() {
    let db = make_db().await;
    common::test_count_by_status(&*db).await;
}

#[tokio::test]
#[ignore]
async fn child_tasks() {
    let db = make_db().await;
    common::test_child_tasks(&*db).await;
}

#[tokio::test]
#[ignore]
async fn claude_run_lifecycle() {
    let db = make_db().await;
    common::test_claude_run_lifecycle(&*db).await;
}

#[tokio::test]
#[ignore]
async fn claim_empty() {
    let db = make_db().await;
    common::test_claim_empty(&*db).await;
}

#[tokio::test]
#[ignore]
async fn stale_runs() {
    let db = make_db().await;
    common::test_stale_runs(&*db).await;
}

#[tokio::test]
#[ignore]
async fn task_links() {
    let db = make_db().await;
    common::test_task_links(&*db).await;
}

#[tokio::test]
#[ignore]
async fn task_prs() {
    let db = make_db().await;
    common::test_task_prs(&*db).await;
}

#[tokio::test]
#[ignore]
async fn attachments() {
    let db = make_db().await;
    common::test_attachments(&*db).await;
}

#[tokio::test]
#[ignore]
async fn api_keys() {
    let db = make_db().await;
    common::test_api_keys(&*db).await;
}

#[tokio::test]
#[ignore]
async fn sprint_crud() {
    let db = make_db().await;
    common::test_sprint_crud(&*db).await;
}

#[tokio::test]
#[ignore]
async fn subtask_workflow() {
    let db = make_db().await;
    common::test_subtask_workflow(&*db).await;
}

#[tokio::test]
#[ignore]
async fn update_task_no_changes() {
    let db = make_db().await;
    common::test_update_task_no_changes(&*db).await;
}

#[tokio::test]
#[ignore]
async fn list_tasks_combined_filters() {
    let db = make_db().await;
    common::test_list_tasks_combined_filters(&*db).await;
}

#[tokio::test]
#[ignore]
async fn update_project_all_fields() {
    let db = make_db().await;
    common::test_update_project_all_fields(&*db).await;
}

#[tokio::test]
#[ignore]
async fn update_project_no_changes() {
    let db = make_db().await;
    common::test_update_project_no_changes(&*db).await;
}

#[tokio::test]
#[ignore]
async fn get_project_by_slug_not_found() {
    let db = make_db().await;
    common::test_get_project_by_slug_not_found(&*db).await;
}

#[tokio::test]
#[ignore]
async fn task_filter_by_sprint_id() {
    let db = make_db().await;
    common::test_task_filter_by_sprint_id(&*db).await;
}

#[tokio::test]
#[ignore]
async fn update_sprint_no_changes() {
    let db = make_db().await;
    common::test_update_sprint_no_changes(&*db).await;
}
