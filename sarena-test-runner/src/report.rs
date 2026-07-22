use sarena_common_test::{
    TestStatus,
    tlv_reader::{LogEntry, TestResult},
};

pub fn print_test_result(test: &TestResult) {
    match test.status {
        TestStatus::Pass => {
            println!("\x1b[32m[PASS]\x1b[0m {}", test.name);
        }
        TestStatus::Fail => {
            println!("\x1b[31m[FAIL]\x1b[0m {}", test.name);
        }
        TestStatus::Skip => {
            println!("\x1b[33m[SKIP]\x1b[0m {}", test.name);
        }
        TestStatus::FrameworkError => {
            println!("\x1b[35m[FRAMEWORK ERROR]\x1b[0m {}", test.name);
            panic!("Unexpected error occurred in test framework");
        }
    }
    for entry in &test.logs {
        println!("{}:{}:\n{}\n", test.file, entry.line, format_log(entry));
    }
    println!("");
}

/// Replace printf-style specifiers with their argument values in order.
/// Supports the same specifiers as bpf_trace_printk.
pub fn format_log(entry: &LogEntry) -> String {
    // Specifiers tried in order; first match at each position wins.
    const SPECS: &[(&str, bool)] = &[
        ("%llu", false),
        ("%lld", true),
        ("%lu", false),
        ("%ld", true),
        ("%u", false),
        ("%d", true),
        ("%llx", false),
        ("%lx", false),
        ("%x", false),
        ("%p", false),
    ];

    let mut out = entry.fmt.clone();
    let mut used = 0usize;

    // Walk the string once, replacing the leftmost specifier each pass
    // until all arguments are consumed.
    while used < entry.args.len() {
        // Find the leftmost specifier
        let best = SPECS
            .iter()
            .filter_map(|&(spec, signed)| out.find(spec).map(|pos| (pos, spec, signed)))
            .min_by_key(|&(pos, _, _)| pos);

        let Some((pos, spec, signed)) = best else {
            break;
        };

        let val = entry.args[used];
        let replacement = if spec == "%p" {
            format!("{:#018x}", val)
        } else if spec.ends_with('x') {
            format!("{:#x}", val)
        } else if signed {
            format!("{}", val as i64)
        } else {
            format!("{}", val)
        };

        out.replace_range(pos..pos + spec.len(), &replacement);
        used += 1;
    }

    out
}
