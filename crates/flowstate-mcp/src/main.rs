mod protocol;
mod server;
mod tools;

use flowstate_service::HttpService;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "flowstate_mcp=info".parse().unwrap()),
        )
        .with_writer(std::io::stderr)
        .init();

    let base_url =
        std::env::var("FLOWSTATE_SERVER_URL").unwrap_or_else(|_| "http://localhost:3710".into());
    let api_key = std::env::var("FLOWSTATE_API_KEY").ok();

    let service = match api_key {
        Some(key) => HttpService::with_api_key(&base_url, key),
        None => HttpService::new(&base_url),
    };

    server::run(service).await
}
