use std::{
    collections::BTreeMap,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

use aya::{
    Ebpf, TestRun, TestRunOptions,
    maps::{Array, MapData},
    programs::SchedClassifier,
};
use regex::Regex;
use sarena_common_test::{ScapyAssert, TEST_RESULT_MAP_SIZE, TestStatus, tlv_reader};

use crate::{Res, TestRunnerError, report};

const PAGE_SIZE: usize = 4096;
const CTX_SIZE: usize = 256;
const HEADROOM: usize = 256;
const TAILROOM: usize = 320;

#[derive(Default)]
struct ProgramSet {
    arrange_name: Option<String>,
    act_name: Option<String>,
    assert_name: Option<String>,
}

#[test]
fn ebpf_test_runner() -> Res<()> {
    println!("\n");
    println!("\x1b[36m===== RUNNING eBPF TESTS =====\x1b[0m");
    println!("\n");

    let dir = std::env::var("EBPF_DIR").unwrap_or_else(|_| "/usr/lib/sarena/ebpf".into());

    let mut prod_bpf = Ebpf::load_file(format!("{dir}/sarena-ebpf-programs.o"))?;
    let mut test_bpf = Ebpf::load_file(format!("{dir}/sarena-ebpf-test-programs.o"))?;

    run_test(&mut test_bpf)?;

    Ok(())
}

fn run_test(test_bpf: &mut Ebpf) -> Res<()> {
    let re = Regex::new(r"^__test_fw_(?P<ptype>arrange|act|assert)_(?P<name>.+)$")?;

    // Collect matching program names grouped by test name.
    // slots: [arrange_prog_name, setup_prog_name, check_prog_name]
    let mut groups: BTreeMap<String, ProgramSet> = BTreeMap::new();
    for (prog_name, _) in test_bpf.programs() {
        if let Some(caps) = re.captures(prog_name) {
            let test_name = caps["name"].to_owned();
            let program_set = groups.entry(test_name.clone()).or_default();

            match &caps["ptype"] {
                "arrange" => {
                    assert!(
                        program_set.arrange_name.is_none(),
                        "multiple arrange programs found for '{test_name}'"
                    );
                    program_set.arrange_name = Some(prog_name.to_owned());
                }
                "act" => {
                    assert!(
                        program_set.act_name.is_none(),
                        "multiple act programs found for '{test_name}'"
                    );
                    program_set.act_name = Some(prog_name.to_owned());
                }
                "assert" => {
                    assert!(
                        program_set.assert_name.is_none(),
                        "multiple assert programs found for '{test_name}'"
                    );
                    program_set.assert_name = Some(prog_name.to_owned());
                }
                _ => {
                    unreachable!();
                }
            };
        }
    }

    // Validate: every test must have a check program.
    for (test_name, program_set) in &groups {
        if program_set.assert_name.is_none() {
            return Err(TestRunnerError::MissingCheck(test_name.to_string()));
        }
    }

    // Load all matched programs.
    for program_set in groups.values() {
        if let Some(name) = &program_set.arrange_name {
            load_bpf_program(test_bpf, name)?;
        }
        if let Some(name) = &program_set.act_name {
            load_bpf_program(test_bpf, name)?;
        }
        if let Some(name) = &program_set.assert_name {
            load_bpf_program(test_bpf, name)?;
        }
    }

    let map_name = "test_suite_result";
    let map = test_bpf
        .take_map(map_name)
        .ok_or_else(|| TestRunnerError::MapNotFound(map_name.to_owned()))?;
    let mut test_suite_result: Array<_, [u8; 8192]> = Array::try_from(map)?;

    let map_name = "test_suite_status_code";
    let map = test_bpf
        .take_map(map_name)
        .ok_or_else(|| TestRunnerError::MapNotFound(map_name.to_owned()))?;
    let mut test_suite_status_code: Array<_, u32> = Array::try_from(map)?;
    test_suite_status_code.set(0, 0, 0)?;

    let map_name = "scapy_assert_map";
    let map = test_bpf
        .take_map(map_name)
        .ok_or_else(|| TestRunnerError::MapNotFound(map_name.to_owned()))?;
    let mut scapy_assert_map: Array<_, ScapyAssert> = Array::try_from(map)?;

    let map_name = "scapy_assert_map_count";
    let map = test_bpf
        .take_map(map_name)
        .ok_or_else(|| TestRunnerError::MapNotFound(map_name.to_owned()))?;
    let mut scapy_assert_map_count: Array<_, u32> = Array::try_from(map)?;

    // Build and run each TestCase.
    for (test_name, program_set) in &groups {
        let arrange_prog = program_set
            .arrange_name
            .as_deref()
            .map(|n| get_sched_classifier(test_bpf, n))
            .transpose()?;
        let act_prog = program_set
            .act_name
            .as_deref()
            .map(|n| get_sched_classifier(test_bpf, n))
            .transpose()?;
        let assert_prog =
            get_sched_classifier(test_bpf, program_set.assert_name.as_deref().unwrap())?;
        sub_test(
            test_name.as_str(),
            &mut scapy_assert_map,
            &mut scapy_assert_map_count,
            &mut test_suite_result,
            &mut test_suite_status_code,
            arrange_prog,
            act_prog,
            assert_prog,
        )?;
    }

    Ok(())
}

fn sub_test(
    name: &str,
    scapy_assert_map: &mut Array<MapData, ScapyAssert>,
    scapy_assert_map_count: &mut Array<MapData, u32>,
    test_suite_result: &mut Array<MapData, [u8; TEST_RESULT_MAP_SIZE]>,
    test_suite_status_code: &mut Array<MapData, u32>,
    arrange_prog: Option<&SchedClassifier>,
    act_prog: Option<&SchedClassifier>,
    assert_prog: &SchedClassifier,
) -> Res<()> {
    let data = vec![0u8; PAGE_SIZE - HEADROOM - TAILROOM];
    let ctx = vec![0u8; CTX_SIZE];

    // Clear test_suite_result
    test_suite_result.set(0, [0u8; TEST_RESULT_MAP_SIZE], 0)?;

    // Clear assert map
    scapy_assert_map_count.set(0, 0u32, 0)?;
    test_suite_status_code.set(0, 0u32, 0)?;

    let (data, ctx) = if let Some(arrange_prog) = arrange_prog {
        let (ret, data, ctx) = run_bpf_program(arrange_prog, &data, &ctx)?;
        if test_error(ret) {
            panic!("[{name}] error while running arrange prog: status code ({ret})");
        }
        (data, ctx)
    } else {
        (data, ctx)
    };

    let (data, ctx) = if let Some(act_prog) = act_prog {
        let (ret, data, ctx) = run_bpf_program(act_prog, &data, &ctx)?;
        if test_error(ret) {
            panic!("[{name}] error while running act prog: status code ({ret})");
        }
        test_suite_status_code.set(0, ret, 0)?;

        (data, ctx)
    } else {
        (data, ctx)
    };

    let (_, _, _) = run_bpf_program(assert_prog, &data, &ctx)?;

    let raw = test_suite_result.get(&0, 0)?;

    // Trim trailing zeroes — the eBPF side does not store a length
    // separately. A second map entry for the length would be cleaner
    // but this is sufficient for a fixed test buffer.
    let written = raw
        .iter()
        .rposition(|&b| b != 0)
        .map(|p| p + 1)
        .unwrap_or(0);

    let raw = raw[..written].to_vec();
    if raw.is_empty() {
        return Err(TestRunnerError::NoResult);
    }

    let result = tlv_reader::parse_test(&raw)?;

    report::print_test_result(&result);

    process_asserts(name, scapy_assert_map, scapy_assert_map_count)?;

    if result.status == TestStatus::Fail {
        panic!("Test failed.");
    }

    Ok(())
}

fn load_bpf_program(bpf: &mut Ebpf, name: &str) -> Res<()> {
    let program = bpf
        .program_mut(name)
        .ok_or_else(|| TestRunnerError::ProgramNotFound(name.to_string()))?;
    let program: &mut SchedClassifier = program.try_into()?;
    program.load()?;
    Ok(())
}

fn get_sched_classifier<'a>(bpf: &'a Ebpf, name: &str) -> Res<&'a SchedClassifier> {
    let sched = bpf
        .program(name)
        .ok_or_else(|| TestRunnerError::ProgramNotFound(name.to_string()))?
        .try_into()?;
    Ok(sched)
}

fn run_bpf_program(
    prog: &SchedClassifier,
    data: &[u8],
    ctx: &[u8],
) -> Res<(u32, Vec<u8>, Vec<u8>)> {
    let mut data_out = vec![0u8; data.len()];
    let mut ctx_out = vec![0u8; ctx.len()];

    let opts = TestRunOptions {
        data_in: Some(data),
        data_out: Some(&mut data_out),
        ctx_in: Some(ctx),
        ctx_out: Some(&mut ctx_out),
        repeat: 1,
        ..Default::default()
    };

    let ret = prog.test_run(opts)?;
    data_out.truncate(ret.data_size_out as usize);
    ctx_out.truncate(ret.ctx_size_out as usize);

    Ok((ret.return_value, data_out, ctx_out))
}

const SCAPY_TRACE_DIFF_CMD: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../scapy/trace_diff_pkts.py");
// The script's `#!/usr/bin/env python3` shebang needs the venv's python3 (with
// scapy installed) ahead of the system one on PATH — mirrors the justfile.
const SCAPYENV_BIN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../scapyenv/bin");

fn process_asserts(
    name: &str,
    scapy_assert_map: &mut Array<MapData, ScapyAssert>,
    scapy_assert_map_count: &mut Array<MapData, u32>,
) -> Res<()> {
    let count = scapy_assert_map_count.get(&0, 0)?;
    if count == 0 {
        return Ok(());
    }

    let asserts: Vec<ScapyAssert> = (0..count)
        .map(|i| scapy_assert_map.get(&i, 0))
        .collect::<Result<_, _>>()?;
    let json_bytes = serde_json::to_vec(&asserts)?;

    println!("[{name}]  Scapy asserts failed: {count}");

    let path = std::env::var_os("PATH").unwrap_or_default();
    let path = std::env::join_paths(
        std::iter::once(PathBuf::from(SCAPYENV_BIN)).chain(std::env::split_paths(&path)),
    )
    .expect("scapyenv bin path and existing PATH must be joinable");

    let mut child = Command::new(SCAPY_TRACE_DIFF_CMD)
        .env("PATH", path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    child
        .stdin
        .take()
        .expect("child stdin was piped")
        .write_all(&json_bytes)?;

    let output = child.wait_with_output()?;
    if !output.status.success() {
        panic!(
            "error while tracing diff pkts: exited with {}",
            output.status
        );
    }

    println!(
        "\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    Ok(())
}

fn test_error(ret: u32) -> bool {
    return ret == TestStatus::Fail as u32 || ret == TestStatus::FrameworkError as u32;
}
