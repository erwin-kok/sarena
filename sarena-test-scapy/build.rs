use std::{env, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=generator");

    let out_dir = env::var("OUT_DIR").unwrap();
    let output = format!("{out_dir}/scapy_bytes.rs");

    let status = Command::new("python3")
        .arg("../scapy/scapy_header_gen.py")
        .arg(&output)
        .status()
        .expect("failed to run scapy generator");

    println!("cargo:warning=Generated scapy bytes at {output}");

    assert!(status.success());
}
