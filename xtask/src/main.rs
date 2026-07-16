mod build_ebpf;
mod install_ebpf;

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
    BuildEbpf(build_ebpf::BuildEbpfOptions),

    /// Install compiled eBPF objects to a system or development directory.
    ///
    /// Typical development workflow:
    ///   cargo xtask build-ebpf
    ///   cargo xtask install-ebpf --dir /usr/lib/myproject/ebpf
    ///
    /// Your userspace binary then loads them at runtime with:
    ///   Bpf::load_file("/usr/lib/myproject/ebpf/my-prog.o")?
    InstallEbpf(install_ebpf::InstallEbpfOptions),
}

fn main() -> Result<()> {
    let XtaskOptions { command } = Parser::parse();
    match command {
        Subcommand::BuildEbpf(opts) => build_ebpf::run(opts),
        Subcommand::InstallEbpf(opts) => install_ebpf::run(opts),
    }
}
