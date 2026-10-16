#![no_main]

use std::io::Write;

use libfuzzer_sys::fuzz_target;

// Log line reading: a file of arbitrary bytes must never panic, in either
// format, and can never report more lines than the file has newlines.
fuzz_target!(|data: &[u8]| {
    let Ok(mut file) = tempfile::NamedTempFile::new() else {
        return;
    };
    if file.write_all(data).is_err() || file.flush().is_err() {
        return;
    }
    let path = file.path().to_path_buf();
    let newline_bound = data.iter().filter(|&&byte| byte == b'\n').count() as u64 + 1;

    let mut text_lines = 0u64;
    let _ = log_bill::read::visit_text(&path, |_line| text_lines += 1);
    assert!(
        text_lines <= newline_bound,
        "text mode produced {text_lines} lines for {newline_bound} newlines"
    );

    let mut json_lines = 0u64;
    let _ = log_bill::read::visit_jsonl(&path, None, |_line| json_lines += 1);
    assert!(
        json_lines <= newline_bound,
        "jsonl mode produced {json_lines} lines for {newline_bound} newlines"
    );

    let mut named_lines = 0u64;
    let _ = log_bill::read::visit_jsonl(&path, Some("note"), |_line| named_lines += 1);
    assert!(
        named_lines <= newline_bound,
        "jsonl mode with a message field produced {named_lines} lines for {newline_bound} newlines"
    );
});
