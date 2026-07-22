#[macro_export]
macro_rules! status {
    ($t:ident, $status:expr) => {{
        $t.set_status($status);
    }};
}

#[macro_export]
macro_rules! assert_buffer {
    ($t:ident, $ctx:expr, $name:literal, $first_layer:literal, $offset:expr, $buf:expr, $len:expr) => {{
        $t.assert_buffer_inner(
            file!(),
            line!(),
            $ctx,
            $name,
            $first_layer,
            $offset,
            $buf,
            $len,
            concat!("Buffer '", stringify!($buf), "' of len (%d) < LEN (%d)"),
            concat!("CTX and buffer '", stringify!($buf), "' content mismatch"),
        )
    }};
}

#[macro_export]
macro_rules! test_log {
    // Zero args
    ($t:ident, $fmt:literal) => {{
        $t.log0(line!(), $fmt.as_bytes());
    }};
    // One arg
    ($t:ident, $fmt:literal, $a0:expr) => {{
        $t.log1(line!(), $fmt.as_bytes(), $a0 as u64);
    }};
    // Two args
    ($t:ident, $fmt:literal, $a0:expr, $a1:expr) => {{
        $t.log2(line!(), $fmt.as_bytes(), $a0 as u64, $a1 as u64);
    }};
    // Three args
    ($t:ident, $fmt:literal, $a0:expr, $a1:expr, $a2:expr) => {{
        $t.log3(line!(), $fmt.as_bytes(), $a0 as u64, $a1 as u64, $a2 as u64);
    }};
    // Four or more args — falls back to generic log with a slice
    ($t:ident, $fmt:literal, $($arg:expr),+ $(,)?) => {{
        $t.log(line!(), $fmt.as_bytes(), &[$($arg as u64),+]);
    }};
}

#[macro_export]
macro_rules! test_fatal {
    ($t:ident, $fmt:expr $(, $arg:expr)* $(,)?) => {{
        $t.log(line!(), $fmt.as_bytes(), &[$($arg as u64),*]);
        $t.fail();
        break;
    }};
}

#[macro_export]
macro_rules! test_skip {
    ($t:ident) => {{
        $t.skip();
        break;
    }};
}

#[macro_export]
macro_rules! assert_test {
    ($t:ident, $cond:expr) => {{
        if !($cond) {
            $crate::test_fatal!($t, concat!("assert failed: ", stringify!($cond)));
        }
    }};
    ($t:ident, $cond:expr, $fmt:literal $(, $arg:expr)* $(,)?) => {{
        if !($cond) {
            $crate::test_fatal!($t, $fmt $(, $arg)*);
        }
    }};
}
