#[tokio::main]
async fn main() -> std::io::Result<()> {
    mobile_mcp_server::stdio::serve().await
}
