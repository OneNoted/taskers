#[tokio::main]
async fn main() -> anyhow::Result<()> {
    taskers_cli::run().await
}
