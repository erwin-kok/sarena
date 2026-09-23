use std::{
    env,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

use sha2::{Digest as _, Sha256};

/// (object name, generated constant)
const OBJECTS: &[(&str, &str)] = &[
    ("sarena-ebpf-programs", "PROGRAMS"),
    ("sarena-ebpf-test-programs", "TEST_PROGRAMS"),
];

fn main() {
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let object_dir = object_dir();

    let mut generated = String::new();
    for (name, constant) in OBJECTS {
        let src = object_dir.join(format!("{name}.o"));
        println!("cargo:rerun-if-changed={}", src.display());
        let bytes = fs::read(&src).unwrap_or_else(|e| {
            panic!(
                "the eBPF object `{}` could not be read: {e}\n Run `just build-ebpf`first.",
                src.display()
            )
        });
        let sha256 = hex::encode(Sha256::digest(&bytes));
        let file = format!("{name}.o");
        fs::write(out_dir.join(&file), &bytes).unwrap();
        write!(
            generated,
            "pub static {constant}: EbpfObject = EbpfObject {{\n    \
             name: {name:?},\n    \
             bytes: aya::include_bytes_aligned!(concat!(env!(\"OUT_DIR\"), \"/{file}\")),\n    \
             sha256: {sha256:?},\n\
             }};\n"
        )
        .unwrap();
    }

    fs::write(out_dir.join("objects.rs"), generated).unwrap();
}

fn object_dir() -> PathBuf {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    workspace_root(&manifest_dir).join("target-ebpf")
}

fn workspace_root(manifest_dir: &Path) -> PathBuf {
    manifest_dir
        .parent()
        .expect("sarena-ebpf-objects lives directly under the workspace root")
        .to_path_buf()
}
