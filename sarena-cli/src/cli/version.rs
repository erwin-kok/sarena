use anyhow::Result;

use crate::app::App;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const GIT_HASH: &str = env!("SARENA_CLI_GIT_HASH");
const BUILD_DATE: &str = env!("SARENA_CLI_BUILD_DATE");

pub async fn run(app: &App) -> Result<()> {
    let debuginfo = app.client.daemon().debuginfo().await?;

    println!("client version: {VERSION} ({GIT_HASH}, built {BUILD_DATE})");
    println!("daemon version: {}", debuginfo.version);
    Ok(())
}
