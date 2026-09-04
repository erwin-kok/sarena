use std::env::consts;

use anyhow::Result;
use sarena_api_types_v1::daemon::SarenaVersion;
use serde::Serialize;

use crate::{app::App, cli::OutputFormat, output::print_output};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const GIT_HASH: &str = env!("SARENA_CLI_GIT_HASH");
const BUILD_DATE: &str = env!("SARENA_CLI_BUILD_DATE");

#[derive(Serialize)]
struct VersionInfo {
    client: SarenaVersion,
    daemon: SarenaVersion,
}

pub async fn run(app: &App, format: Option<&OutputFormat>) -> Result<()> {
    let client_version = SarenaVersion {
        version: VERSION.to_owned(),
        git_hash: GIT_HASH.to_owned(),
        build_date: BUILD_DATE.to_owned(),
        os: consts::OS.to_owned(),
        arch: consts::ARCH.to_owned(),
    };

    let debuginfo = app.client.daemon().debuginfo().await?;

    if let Some(f) = format {
        print_output(
            &VersionInfo {
                client: client_version,
                daemon: debuginfo.version,
            },
            f,
        )?;
    } else {
        println!("client version: {}", version(&client_version));
        println!("daemon version: {}", version(&debuginfo.version));
    }

    Ok(())
}

fn version(version: &SarenaVersion) -> String {
    format!(
        "{} ({}, built {}, {}/{})",
        version.version, version.git_hash, version.build_date, version.os, version.arch
    )
}
