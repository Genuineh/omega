use std::io;
use std::sync::Arc;

use omega_core::{DynLlmClient, MinimaxClient, MinimaxConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = MinimaxConfig::from_env().map_err(|e| anyhow::anyhow!("{e}"))?;
    let client: DynLlmClient =
        Arc::new(MinimaxClient::new(config).map_err(|e| anyhow::anyhow!("{e}"))?);

    let cwd = std::env::current_dir()?;
    let system = format!(
        "You are a coding agent at {}. Use bash to solve tasks. Act, don't explain.",
        cwd.display()
    );

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();

    omega_repl::run_repl(&mut reader, &mut writer, client, cwd, system).await
}
