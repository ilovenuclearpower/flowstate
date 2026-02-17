use std::net::SocketAddr;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tokio::net::TcpListener;

use flowstate_server::auth;

#[derive(Parser)]
#[command(name = "flowstate-server")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate a new API key
    Keygen {
        /// Human-readable name for the key
        #[arg(long, default_value = "")]
        name: String,
    },
    /// List all API keys (metadata only, no secrets)
    ListKeys,
    /// Revoke (delete) an API key by ID
    RevokeKey {
        /// The API key ID to revoke
        id: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let db = flowstate_db::Db::open_default()?;

    match cli.command {
        Some(Commands::Keygen { name }) => {
            let raw_key = auth::generate_api_key();
            let hash = auth::sha256_hex(&raw_key);
            let api_key = db.insert_api_key(&name, &hash)?;
            eprintln!("Created API key (id: {})", api_key.id);
            if !name.is_empty() {
                eprintln!("  name: {name}");
            }
            // Print the raw key to stdout so it can be captured
            println!("{raw_key}");
            eprintln!("\nSave this key — it cannot be retrieved again.");
        }
        Some(Commands::ListKeys) => {
            let keys = db.list_api_keys()?;
            if keys.is_empty() {
                eprintln!("No API keys found.");
            } else {
                println!("{:<38} {:<20} {:<28} LAST USED", "ID", "NAME", "CREATED");
                for key in keys {
                    println!(
                        "{:<38} {:<20} {:<28} {}",
                        key.id,
                        if key.name.is_empty() { "-" } else { &key.name },
                        key.created_at,
                        key.last_used_at.as_deref().unwrap_or("never"),
                    );
                }
            }
        }
        Some(Commands::RevokeKey { id }) => {
            db.delete_api_key(&id)?;
            eprintln!("Revoked API key {id}");
        }
        None => {
            // Default: start server
            let bind = std::env::var("FLOWSTATE_BIND").unwrap_or_else(|_| "0.0.0.0".into());
            let port: u16 = std::env::var("FLOWSTATE_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(3710);

            let addr = SocketAddr::new(bind.parse()?, port);

            let auth = auth::build_auth_config(&db);
            if auth.is_some() {
                eprintln!("authentication enabled");
            } else {
                eprintln!("authentication disabled (no FLOWSTATE_API_KEY or DB keys)");
            }

            let listener = TcpListener::bind(addr).await?;
            eprintln!("flowstate-server listening on http://{addr}");

            flowstate_server::serve(listener, db, auth).await?;
        }
    }

    Ok(())
}
