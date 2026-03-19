use std::sync::Arc;

use omega_core::{DynLlmClient, MinimaxClient, MinimaxConfig};
use omega_observability::init_tracing_channel;
use omega_tui::{run, TuiLaunchConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = MinimaxConfig::from_env().map_err(|e| anyhow::anyhow!("{e}"))?;
    let model_name = config.model.clone();
    let client: DynLlmClient =
        Arc::new(MinimaxClient::new(config).map_err(|e| anyhow::anyhow!("{e}"))?);

    let cwd = std::env::current_dir()?;
    let system = format!(
        "You are a coding agent at {}. Use bash to solve tasks. Act, don't explain.",
        cwd.display()
    );
    let trace_rx = init_tracing_channel()?;

    run(TuiLaunchConfig {
        client,
        cwd,
        model_name,
        runtime_handle: tokio::runtime::Handle::current(),
        system,
        trace_rx,
    })
}
