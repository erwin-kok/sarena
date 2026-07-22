use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, Result, bail};
use clap::Parser;

use crate::build_ebpf::{self, BuildEbpfOptions};

/// Default source directory (where `build-ebpf` puts its output).
const DEFAULT_SRC_DIR: &str = "./target-ebpf";

/// Default install prefix, mirrors what most Linux packages use.
const DEFAULT_INSTALL_DIR: &str = "/usr/lib/sarena/ebpf";

#[derive(Debug, Default, Parser)]
pub struct InstallEbpfOptions {
    /// Source directory containing the compiled .o files.
    ///
    /// This is normally the --out-dir you passed to build-ebpf (default:
    /// ./target-ebpf).
    #[clap(long, default_value = DEFAULT_SRC_DIR)]
    pub src_dir: PathBuf,

    /// Destination directory where the .o files will be installed.
    ///
    /// Your userspace binary should read this path from an env var or config
    /// file so it can be overridden without recompilation.  A common pattern:
    ///
    ///   let dir = std::env::var("EBPF_DIR")
    ///       .unwrap_or_else(|_| "/usr/lib/myproject/ebpf".into());
    ///   let bpf = Bpf::load_file(format!("{dir}/{name}.o"))?;
    #[clap(long, default_value = DEFAULT_INSTALL_DIR)]
    pub dir: PathBuf,

    /// Install only the named object(s).  Can be repeated.
    /// If omitted, every .o file in --src-dir is installed.
    #[clap(long = "object", short = 'o', value_name = "NAME")]
    pub objects: Vec<String>,

    /// Do not overwrite existing files.
    ///
    /// By default, install-ebpf overwrites whatever is already at the
    /// destination — this is the right behaviour for iterative development.
    /// Pass --no-clobber in packaging or CI scripts where a pre-existing file
    /// should be treated as an error.
    #[clap(long)]
    pub no_clobber: bool,
}

pub(crate) fn run(opts: InstallEbpfOptions) -> Result<()> {
    // Always build ebpf first
    build_ebpf::run(BuildEbpfOptions::default())?;

    let src_dir = &opts.src_dir;

    if !src_dir.exists() {
        bail!(
            "source directory `{}` does not exist — run `cargo xtask build-ebpf` first",
            src_dir.display()
        );
    }

    // Collect the list of .o files to install.
    let candidates: Vec<PathBuf> = if opts.objects.is_empty() {
        collect_objects(src_dir)?
    } else {
        let mut paths = Vec::new();
        for name in &opts.objects {
            let name = ensure_o_extension(name);
            let path = src_dir.join(&name);
            if !path.exists() {
                bail!(
                    "requested object `{name}` not found in `{}`",
                    src_dir.display()
                );
            }
            paths.push(path);
        }
        paths
    };

    if candidates.is_empty() {
        bail!(
            "no .o files found in `{}` — run `cargo xtask build-ebpf` first",
            src_dir.display()
        );
    }

    fs::create_dir_all(&opts.dir)
        .with_context(|| format!("failed to create `{}`", opts.dir.display()))?;

    let mut installed = 0usize;
    for src in &candidates {
        let file_name = src.file_name().expect("collected path has no filename");
        let dst = opts.dir.join(file_name);

        if dst.exists() && opts.no_clobber {
            bail!(
                "destination `{}` already exists — remove --no-clobber to overwrite",
                dst.display()
            );
        }

        fs::copy(src, &dst)
            .with_context(|| format!("failed to copy `{}` → `{}`", src.display(), dst.display()))?;

        println!("  installed `{}` → `{}`", src.display(), dst.display());
        installed += 1;
    }

    println!(
        "\nInstalled {installed} object(s) to `{}`.",
        opts.dir.display()
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Collect all `.o` files directly inside `dir` (non-recursive — each eBPF
/// package produces exactly one object at the top level).
fn collect_objects(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("failed to read `{}`", dir.display()))? {
        let entry =
            entry.with_context(|| format!("failed to read entry in `{}`", dir.display()))?;
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("o") {
            out.push(path);
        }
    }
    out.sort(); // deterministic order
    Ok(out)
}

/// Ensure a user-supplied object name ends in `.o`.
fn ensure_o_extension(name: &str) -> String {
    if name.ends_with(".o") {
        name.to_string()
    } else {
        format!("{name}.o")
    }
}
