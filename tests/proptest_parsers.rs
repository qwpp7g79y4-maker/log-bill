//! Property tests: every parser that reads untrusted input must not panic,
//! whatever bytes or text it is given.

use std::io::Write;

use log_bill::{diff, read, rules};
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// The rules-file parser never panics on arbitrary bytes.
    #[test]
    fn parse_rules_never_panics(data in prop::collection::vec(any::<u8>(), 0..512)) {
        let _ = rules::parse_rules(&data);
    }

    /// The `--max-new-share` percentage parser never panics on arbitrary text.
    #[test]
    fn parse_percent_never_panics(text in ".{0,100}") {
        let _ = diff::parse_percent(&text);
    }

    /// Reading a file of arbitrary bytes, in either format, never panics.
    #[test]
    fn read_never_panics(data in prop::collection::vec(any::<u8>(), 0..4096)) {
        let mut file = tempfile::NamedTempFile::new().expect("create a temp file");
        file.write_all(&data).expect("write the sample");
        file.flush().expect("flush the sample");
        let path = file.path();

        let _ = read::visit_text(path, |_line| {});
        let _ = read::visit_jsonl(path, None, |_line| {});
        let _ = read::visit_jsonl(path, Some("note"), |_line| {});
    }
}
