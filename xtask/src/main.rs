mod build_ebpf;

use anyhow::Result;
use clap::Parser;

#[derive(Debug, Parser)]
#[clap(name = "xtask", about = "Build automation for the workspace")]
pub struct XtaskOptions {
    #[clap(subcommand)]
    command: Subcommand,
}

#[derive(Debug, Parser)]
enum Subcommand {
    /// Compile eBPF programs and place them in ./target-ebpf.
    ///
    /// From there, `sarena-ebpf-objects` embeds them into the userspace
    /// binaries.
    BuildEbpf(build_ebpf::BuildEbpfOptions),
}

fn main() -> Result<()> {
    let XtaskOptions { command } = Parser::parse();
    match command {
        Subcommand::BuildEbpf(opts) => build_ebpf::run(opts),
    }
}
