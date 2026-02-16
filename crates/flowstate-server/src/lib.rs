pub mod auth;
pub mod claude;
mod routes;

use std::sync::Arc;

use anyhow::Result;
use flowstate_db::Db;
use flowstate_service::LocalService;
use tokio::net::TcpListener;

use auth::AuthConfig;

pub async fn serve(listener: TcpListener, db: Db, auth: Option<Arc<AuthConfig>>) -> Result<()> {
    let service = LocalService::new(db.clone());
    let app = routes::build_router(service, db, auth);
    axum::serve(listener, app).await?;
    Ok(())
}
