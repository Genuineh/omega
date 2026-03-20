#[tokio::main]
async fn main() -> anyhow::Result<()> {
    omega_app::run().await
}
