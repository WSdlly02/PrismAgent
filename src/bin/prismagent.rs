use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    prismagent::shell::tui::run().await
}
