use perpetua_core::MinimalCore;
use perpetua_server::build_router;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let core = MinimalCore::new();
    let app = build_router(core);
    let bind_addr = std::env::var("PERPETUA_BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:4850".to_string());
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    tracing::info!("Perpetua Broadcast Automation Server listening on http://{}", bind_addr);

    axum::serve(listener, app).await?;
    Ok(())
}
