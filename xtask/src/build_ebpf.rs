use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context as _, Result, bail};
use cargo_metadata::MetadataCommand;
use clap::Parser;
use serde_json::Value;

/// Default eBPF packages to build. Can be overridden via --package.
const DEFAULT_EBPF_PACKAGES: &[&str] = &["dataplane-ebpf-programs", "dataplane-ebpf-test-programs"];

/// Default target triple for eBPF programs.
const DEFAULT_TARGET: &str = "bpfel-unknown-none";

#[derive(Debug, Parser)]
pub struct BuildEbpfOptions {
    /// Target triple for the eBPF programs.
    #[clap(long, default_value = DEFAULT_TARGET)]
    pub target: String,

    /// Override which packages to build. Can be repeated: -p foo -p bar.
    /// Defaults to the built-in EBPF_PACKAGES list.
    #[clap(long = "package", short = 'p', value_name = "PKG")]
    pub packages: Vec<String>,

    /// Rust toolchain to use (e.g. "nightly", "nightly-2024-01-01").
    /// Must be nightly because of -Z build-std.
    #[clap(long, default_value = "nightly")]
    pub toolchain: String,

    /// Output directory for the compiled eBPF objects.
    #[clap(long, default_value = "./target-ebpf")]
    pub out_dir: PathBuf,
}

impl Default for BuildEbpfOptions {
    fn default() -> Self {
        Self {
            target: DEFAULT_TARGET.to_string(),
            packages: vec![],
            toolchain: "nightly".to_string(),
            out_dir: PathBuf::from("./target-ebpf"),
        }
    }
}

pub(crate) fn run(opts: BuildEbpfOptions) -> Result<()> {
    // Resolve which packages to build.
    let packages_to_build: Vec<String> = if opts.packages.is_empty() {
        DEFAULT_EBPF_PACKAGES
            .iter()
            .map(|s| s.to_string())
            .collect()
    } else {
        opts.packages.clone()
    };

    // `cargo metadata` gives us the workspace root (so paths are always
    // correct regardless of where xtask is invoked from) and lets us validate
    // the requested package names up front.
    let metadata = MetadataCommand::new()
        .no_deps()
        .exec()
        .context("failed to run `cargo metadata`")?;

    let workspace_root = metadata.workspace_root.as_std_path().to_path_buf();

    let workspace_package_names: HashSet<String> = metadata
        .packages
        .iter()
        .map(|p| p.name.to_string())
        .collect();

    for pkg in &packages_to_build {
        if !workspace_package_names.contains(pkg.as_str()) {
            eprintln!(
                "warning: package `{pkg}` was not found in the workspace — \
                 it may fail to build"
            );
        }
    }

    let mut built_artifacts: Vec<(String, PathBuf)> = Vec::new();

    for pkg in &packages_to_build {
        println!("==> Building eBPF package `{pkg}`");

        let artifacts = build_package(pkg, &opts.target, &opts.toolchain, &workspace_root)
            .with_context(|| format!("failed to build package `{pkg}`"))?;

        if artifacts.is_empty() {
            bail!(
                "package `{pkg}` built successfully but produced no artifacts.\n\
                 \n\
                 Possible causes:\n\
                 • The package has no [[bin]] target (add one in its Cargo.toml)\n\
                 • The binary name differs from the package name\n\
                 • The build was cached — try `cargo clean -p {pkg}` and retry"
            );
        }

        println!("  produced {} artifact(s) for `{pkg}`", artifacts.len());
        for a in &artifacts {
            println!("    {}", a.display());
        }

        for a in artifacts {
            built_artifacts.push((pkg.clone(), a));
        }
    }

    copy_artifacts(&built_artifacts, &opts.out_dir).context("failed to copy eBPF artifacts")?;

    println!(
        "\nDone. {} object(s) written to `{}`.",
        built_artifacts.len(),
        opts.out_dir.display()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Build
// ---------------------------------------------------------------------------

/// Compile one package for the eBPF target and return the **executable**
/// paths that cargo reports via its JSON message stream.
///
/// We use the `executable` field of `compiler-artifact` messages rather than
/// `filenames`, because `filenames` also contains `.d` depfiles and `.rlib`
/// intermediates.  The `executable` field is set only for `[[bin]]` targets
/// and is exactly the ELF file we want to hand to Aya.
fn build_package(
    pkg: &str,
    target: &str,
    toolchain: &str,
    workspace_root: &Path,
) -> Result<Vec<PathBuf>> {
    let toolchain_arg = format!("+{toolchain}");

    // Build the argument list.  We keep it as Vec<String> so we can push
    // conditionally without lifetime headaches.
    let args: Vec<String> = vec![
        toolchain_arg,
        "build".into(),
        "-p".into(),
        pkg.into(),
        "--profile".into(),
        "ebpf".into(),
        "--target".into(),
        target.into(),
        "-Z".into(),
        "build-std=core".into(),
        // json-render-diagnostics: JSON on stdout, human-readable diagnostics
        // on stderr — best of both worlds.
        "--message-format=json-render-diagnostics".into(),
    ];

    let mut child = Command::new("cargo")
        .args(&args)
        .current_dir(workspace_root)
        .stderr(Stdio::inherit()) // compiler errors stream straight to terminal
        .stdout(Stdio::piped())
        .spawn()
        .context("failed to spawn `cargo build`")?;

    let stdout = child
        .stdout
        .take()
        .expect("stdout was piped but unavailable");

    let reader = BufReader::new(stdout);
    let mut artifacts: Vec<PathBuf> = Vec::new();

    for line in reader.lines() {
        let line = line.context("error reading cargo stdout")?;

        let v: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue, // skip non-JSON lines (rare with json-render-diagnostics)
        };

        match v["reason"].as_str() {
            Some("compiler-artifact") => {
                // Guard: only collect artifacts that belong to the package we
                // asked to build.  The `target.name` field holds the crate
                // name (e.g. "dataplane-ebpf-programs"), which is stable and
                // unambiguous.  We also check `package_id` as a fallback for
                // workspaces where the target name was customised.
                let target_name = v["target"]["name"].as_str().unwrap_or("");
                let package_id = v["package_id"].as_str().unwrap_or("");
                let is_our_pkg = target_name == pkg
                    || package_id.starts_with(&format!("{pkg} "))
                    || package_id.starts_with(&format!("{pkg}@"));

                if !is_our_pkg {
                    continue;
                }

                // `executable` is present for [[bin]] targets and is the
                // single ELF file we want.  It is absent for rlib/cdylib etc.
                if let Some(exe) = v["executable"].as_str() {
                    artifacts.push(PathBuf::from(exe));
                }
            }

            Some("build-finished") => {
                if v["success"].as_bool() == Some(false) {
                    bail!("cargo reported build-finished with success=false");
                }
            }

            _ => {}
        }
    }

    let status = child.wait().context("failed to wait for `cargo build`")?;
    if !status.success() {
        bail!("`cargo build` exited with {status}");
    }

    Ok(artifacts)
}

// ---------------------------------------------------------------------------
// Copy
// ---------------------------------------------------------------------------

/// Copy every artifact into `out_dir` as `<pkg>.o` (or `<pkg>_<n>.o` for
/// packages that produce multiple binaries).
fn copy_artifacts(artifacts: &[(String, PathBuf)], out_dir: &Path) -> Result<()> {
    fs::create_dir_all(out_dir)
        .with_context(|| format!("failed to create `{}`", out_dir.display()))?;

    let mut seen: HashMap<&str, usize> = HashMap::new();

    for (pkg, src) in artifacts {
        if !src.exists() {
            bail!(
                "artifact `{}` reported by cargo does not exist on disk",
                src.display()
            );
        }

        let idx = {
            let e = seen.entry(pkg.as_str()).or_insert(0);
            *e += 1;
            *e
        };

        let dst_name = if idx == 1 {
            format!("{pkg}.o")
        } else {
            format!("{pkg}_{idx}.o")
        };
        let dst = out_dir.join(&dst_name);

        fs::copy(src, &dst)
            .with_context(|| format!("failed to copy `{}` → `{}`", src.display(), dst.display()))?;

        println!("  copied  `{}` → `{}`", src.display(), dst.display());
    }

    Ok(())
}
