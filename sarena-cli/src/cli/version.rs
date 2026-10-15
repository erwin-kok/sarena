use std::env::consts;

use anyhow::Result;
use sarena_api_types_v1::daemon::SarenaVersion;
use sarena_utils::{
    cli_output::{self, OutputFormat},
    version,
};
use serde::Serialize;

use crate::app::App;

#[derive(Serialize)]
struct VersionInfo {
    client: SarenaVersion,
    daemon: SarenaVersion,
}

pub async fn run(app: &App, format: Option<&OutputFormat>) -> Result<()> {
    let client_version = SarenaVersion {
        version: version::VERSION.to_owned(),
        git_hash: version::GIT_HASH.to_owned(),
        build_date: version::BUILD_DATE.to_owned(),
        os: consts::OS.to_owned(),
        arch: consts::ARCH.to_owned(),
    };

    let debuginfo = app.client.daemon().debuginfo().await?;

    if let Some(f) = format {
        cli_output::print_output(
            &VersionInfo {
                client: client_version,
                daemon: debuginfo.version,
            },
            f,
        )?;
    } else {
        println!("client version: {client_version}");
        println!("daemon version: {}", debuginfo.version);
    }

    Ok(())
}
