pub mod auth;
pub mod claude;
pub mod crypto;
mod routes;

use std::sync::Arc;

use anyhow::Result;
use flowstate_db::Db;
use flowstate_service::LocalService;
use tokio::net::TcpListener;

use auth::AuthConfig;

pub async fn serve(listener: TcpListener, db: Db, auth: Option<Arc<AuthConfig>>) -> Result<()> {
    let encryption_key = crypto::load_or_generate_key();
    let service = LocalService::new(db.clone());
    let app = routes::build_router(service, db, auth, encryption_key);
    axum::serve(listener, app).await?;
    Ok(())
}
