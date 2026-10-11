use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::ops::Index;
use std::path::PathBuf;
use std::process::{Command, Stdio};

// Integration tests are a separate crate and do not see the binary's dependencies.
// serde_json therefore stays with the binary. The tree comes from python3,
// the same program as `python3 -m json.tool`.
const BOOM: &str = "\
import json, sys

def emit(value):
    if value is None:
        out(b'N\\n')
    elif isinstance(value, bool):
        out(b'B1\\n' if value else b'B0\\n')
    elif isinstance(value, int):
        out(('I%d\\n' % value).encode())
    elif isinstance(value, str):
        raw = value.encode()
        out(('S%d\\n' % len(raw)).encode())
        out(raw)
    elif isinstance(value, list):
        out(('A%d\\n' % len(value)).encode())
        for item in value:
            emit(item)
    elif isinstance(value, dict):
        out(('O%d\\n' % len(value)).encode())
        for key, item in value.items():
            raw = key.encode()
            out(('K%d\\n' % len(raw)).encode())
            out(raw)
            emit(item)
    else:
        sys.stderr.write('json has_text one_apostrophe kommagetal\\n')
        sys.exit(1)

def out(data):
    sys.stdout.buffer.write(data)

emit(json.load(sys.stdin))
";

#[derive(Clone, Debug, PartialEq)]
enum Node {
    Null,
    Bool(bool),
    Int(i128),
    Str(String),
    Arr(Vec<Node>),
    Obj(BTreeMap<String, Node>),
}

impl Node {
    fn is_null(&self) -> bool {
        matches!(self, Node::Null)
    }

    fn as_array(&self) -> Option<&Vec<Node>> {
        match self {
            Node::Arr(items) => Some(items),
            Node::Null | Node::Bool(_) | Node::Int(_) | Node::Str(_) | Node::Obj(_) => None,
        }
    }

    fn as_object(&self) -> Option<&BTreeMap<String, Node>> {
        match self {
            Node::Obj(map) => Some(map),
            Node::Null | Node::Bool(_) | Node::Int(_) | Node::Str(_) | Node::Arr(_) => None,
        }
    }

    fn get(&self, key: &str) -> Option<&Node> {
        self.as_object().and_then(|map| map.get(key))
    }

    fn equal_to(&self, number: i128) -> bool {
        matches!(self, Node::Int(value) if *value == number)
    }
}

impl Index<&str> for Node {
    type Output = Node;

    fn index(&self, key: &str) -> &Node {
        self.get(key)
            .unwrap_or_else(|| panic!("no key {key} in {self:?}"))
    }
}

impl Index<usize> for Node {
    type Output = Node;

    fn index(&self, index: usize) -> &Node {
        match self.as_array() {
            Some(items) => items
                .get(index)
                .unwrap_or_else(|| panic!("no index {index} in {self:?}")),
            None => panic!("not a list: {self:?}"),
        }
    }
}

impl PartialEq<bool> for Node {
    fn eq(&self, other: &bool) -> bool {
        matches!(self, Node::Bool(value) if value == other)
    }
}

impl PartialEq<&str> for Node {
    fn eq(&self, other: &&str) -> bool {
        matches!(self, Node::Str(value) if value == *other)
    }
}

impl PartialEq<i32> for Node {
    fn eq(&self, other: &i32) -> bool {
        self.equal_to(i128::from(*other))
    }
}

impl PartialEq<i64> for Node {
    fn eq(&self, other: &i64) -> bool {
        self.equal_to(i128::from(*other))
    }
}

impl PartialEq<u32> for Node {
    fn eq(&self, other: &u32) -> bool {
        self.equal_to(i128::from(*other))
    }
}

impl PartialEq<u64> for Node {
    fn eq(&self, other: &u64) -> bool {
        self.equal_to(i128::from(*other))
    }
}

impl PartialEq<usize> for Node {
    fn eq(&self, other: &usize) -> bool {
        match i128::try_from(*other) {
            Ok(number) => self.equal_to(number),
            Err(_) => false,
        }
    }
}

fn bin() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_log-bill"));
    cmd.current_dir(env!("CARGO_MANIFEST_DIR"));
    cmd
}

fn manifest(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn tmp(item_name: &str, bytes: &[u8]) -> PathBuf {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(item_name);
    std::fs::write(&path, bytes).expect("write fixture");
    path
}

fn run_exit(args: &[&str]) -> (i32, String, String) {
    let output = bin().args(args).output().expect("start binary");
    let code = output.status.code().unwrap_or(9);
    let out = String::from_utf8(output.stdout).expect("stdout is utf-8");
    let err = String::from_utf8(output.stderr).expect("stderr is utf-8");
    (code, out, err)
}

fn json(body: &str) -> Node {
    let mut child = Command::new("python3")
        .args(["-c", BOOM])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("python3");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(body.as_bytes())
        .expect("write");
    let done = child.wait_with_output().expect("python3");
    assert!(
        done.status.success(),
        "not JSON ({}): {body}",
        String::from_utf8_lossy(&done.stderr)
    );
    let mut cursor = JsonCursor {
        buf: &done.stdout,
        pos: 0,
    };
    let node = cursor.parse_node();
    assert_eq!(
        cursor.pos,
        cursor.buf.len(),
        "parser did not consume the whole input"
    );
    node
}

struct JsonCursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> JsonCursor<'a> {
    fn line(&mut self) -> &'a [u8] {
        let rest = &self.buf[self.pos..];
        let end = rest
            .iter()
            .position(|byte| *byte == b'\n')
            .unwrap_or_else(|| panic!("truncated tree"));
        let line = &rest[..end];
        self.pos += end + 1;
        line
    }

    fn take_bytes(&mut self, len: usize) -> &'a [u8] {
        let end = self.pos + len;
        assert!(end <= self.buf.len(), "truncated text");
        let bytes = &self.buf[self.pos..end];
        self.pos = end;
        bytes
    }

    fn text_body(&mut self, len: usize) -> String {
        let raw = self.take_bytes(len);
        std::str::from_utf8(raw)
            .unwrap_or_else(|err| panic!("text ({err})"))
            .to_string()
    }

    fn parse_node(&mut self) -> Node {
        let line = self.line().to_vec();
        match line.first().copied() {
            Some(b'N') => Node::Null,
            Some(b'B') => Node::Bool(line.get(1) == Some(&b'1')),
            Some(b'I') => Node::Int(whole(&line[1..])),
            Some(b'S') => Node::Str(self.text_body(count(&line[1..]))),
            Some(b'A') => self.joined_list(count(&line[1..])),
            Some(b'O') => self.object(count(&line[1..])),
            other => panic!("unknown node {other:?}"),
        }
    }

    fn joined_list(&mut self, n: usize) -> Node {
        let mut items = Vec::with_capacity(n);
        for _ in 0..n {
            items.push(self.parse_node());
        }
        Node::Arr(items)
    }

    fn object(&mut self, n: usize) -> Node {
        let mut map = BTreeMap::new();
        for _ in 0..n {
            let line = self.line().to_vec();
            assert_eq!(line.first().copied(), Some(b'K'), "key");
            let key_name = self.text_body(count(&line[1..]));
            map.insert(key_name, self.parse_node());
        }
        Node::Obj(map)
    }
}

fn count(bytes: &[u8]) -> usize {
    let text = std::str::from_utf8(bytes).expect("count");
    text.parse().unwrap_or_else(|_| panic!("count {text}"))
}

fn whole(bytes: &[u8]) -> i128 {
    let text = std::str::from_utf8(bytes).expect("number");
    text.parse().unwrap_or_else(|_| panic!("number {text}"))
}

fn key_list(value: &Node) -> BTreeSet<&str> {
    value
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect()
}

fn has_text(value: &Node, needle: &str) -> bool {
    match value {
        Node::Null | Node::Bool(_) => false,
        Node::Int(number) => number.to_string().contains(needle),
        Node::Str(text_body) => text_body.contains(needle),
        Node::Arr(items) => items.iter().any(|item| has_text(item, needle)),
        Node::Obj(map) => map
            .iter()
            .any(|(key_name, item)| key_name.contains(needle) || has_text(item, needle)),
    }
}

fn valid_via_json_tool(body: &str) {
    let mut child = Command::new("python3")
        .args(["-m", "json.tool"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("python3");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(body.as_bytes())
        .expect("write");
    let done = child.wait_with_output().expect("python3");
    assert!(
        done.status.success(),
        "json.tool: {}",
        String::from_utf8_lossy(&done.stderr)
    );
}

fn snapshot(name: &str, actual: &str) {
    let path = manifest(&format!("tests/snapshots/{name}"));
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read snapshot {} ({err})", path.display()));
    assert_eq!(actual, expected, "snapshot {}", path.display());
}

const ESTIMATE: &str =
    "estimate: extrapolation of one sample with your own prices; not an official rate";

const SCAN_BASIS: &[&str] = &[
    "estimate",
    "inputs",
    "patterns",
    "schema_version",
    "tool",
    "totals",
    "window",
];

const DIFF_BASIS: &[&str] = &[
    "added",
    "changed",
    "estimate",
    "inputs",
    "patterns",
    "removed",
    "schema_version",
    "tool",
    "totals",
    "totals_delta",
    "window",
];

fn set(keys: &'static [&'static str]) -> BTreeSet<&'static str> {
    keys.iter().copied().collect()
}

#[test]
fn snapshot_scan_and_diff() {
    // scan-text.log: two user lines together 108 bytes, `disk full\n` = 10.
    // 108 * 1000 / 118 = 915 remainder 30, 2*30 < 118, so 91.5%.
    // 10 * 1000 / 118 = 84 remainder 88, 2*88 >= 118, so 85 tenths = 8.5%.
    // diff-old: alpha\n twice (12) and beta\n (5) = 17 bytes, 3 lines.
    // diff-new: alpha\n + gamma\n = 12 bytes, 2 lines. Delta -5 bytes, -1 line.
    // alpha 12/17 -> 70.6% to 6/12 -> 50.0%, delta -20.6 percentage points.
    // beta 5/17 -> 29.4% to 0. gamma 0 to 50.0%. New bytes 6 of 12 = 50.0%.
    let (code, scan_out, err) = run_exit(&["scan", "--json", "tests/fixtures/scan-text.log"]);
    assert_eq!(code, 0);
    assert!(err.is_empty(), "{err}");
    assert!(!scan_out.contains("192.0.2.10"), "{scan_out}");
    assert!(!scan_out.contains("2026-10-08T11:07:44Z"), "{scan_out}");
    assert!(!scan_out.contains("user 7"), "{scan_out}");
    assert!(!scan_out.contains("user 8"), "{scan_out}");
    valid_via_json_tool(&scan_out);
    let scan = json(&scan_out);
    assert_eq!(scan["schema_version"], 1);
    assert_eq!(scan["tool"], "log-bill");
    assert!(scan["estimate"].is_null());
    assert!(scan["window"].is_null());
    assert_eq!(key_list(&scan), set(SCAN_BASIS));
    assert_eq!(scan["totals"]["lines"], 3);
    assert_eq!(scan["totals"]["bytes"], 118);
    assert_eq!(scan["patterns"][0]["bytes"], 108);
    assert_eq!(scan["patterns"][0]["lines"], 2);
    assert_eq!(scan["patterns"][0]["share"], "91.5");
    assert_eq!(
        scan["patterns"][0]["text"],
        "user <NUM> failed at <TS> from <IP>"
    );
    assert_eq!(scan["patterns"][1]["bytes"], 10);
    assert_eq!(scan["patterns"][1]["share"], "8.5");
    assert_eq!(scan["inputs"]["paths"][0], "tests/fixtures/scan-text.log");
    snapshot("scan.json", &scan_out);

    let (code, diff_out, err) = run_exit(&[
        "diff",
        "--json",
        "tests/fixtures/diff-old.log",
        "tests/fixtures/diff-new.log",
    ]);
    assert_eq!(code, 0);
    assert!(err.is_empty(), "{err}");
    valid_via_json_tool(&diff_out);
    let diff = json(&diff_out);
    assert_eq!(diff["schema_version"], 1);
    assert_eq!(key_list(&diff), set(DIFF_BASIS));
    assert_eq!(diff["totals"]["old"]["lines"], 3);
    assert_eq!(diff["totals"]["old"]["bytes"], 17);
    assert_eq!(diff["totals"]["new"]["bytes"], 12);
    assert_eq!(diff["totals_delta"]["lines"], -1);
    assert_eq!(diff["totals_delta"]["bytes"], -5);
    assert_eq!(diff["totals"]["added_bytes"], 6);
    assert_eq!(diff["totals"]["added_share"], "50.0");
    assert_eq!(diff["added"][0]["text"], "gamma");
    assert_eq!(diff["added"][0]["bytes"]["delta"], 6);
    assert_eq!(diff["added"][0]["share"]["delta"], "+50.0");
    assert_eq!(diff["removed"][0]["text"], "beta");
    assert_eq!(diff["removed"][0]["bytes"]["delta"], -5);
    assert_eq!(diff["removed"][0]["share"]["old"], "29.4");
    assert_eq!(diff["changed"][0]["text"], "alpha");
    assert_eq!(diff["changed"][0]["bytes"]["old"], 12);
    assert_eq!(diff["changed"][0]["bytes"]["new"], 6);
    assert_eq!(diff["changed"][0]["share"]["delta"], "-20.6");
    let mut combined = diff["added"].as_array().expect("added").clone();
    combined.extend(diff["removed"].as_array().expect("removed").clone());
    combined.extend(diff["changed"].as_array().expect("changed").clone());
    assert_eq!(diff["patterns"].as_array().expect("patterns"), &combined);
    snapshot("diff.json", &diff_out);
}

#[test]
fn costs_only_with_prices_and_window() {
    // 999_999 times 'q' plus `\n` = 1_000_000 bytes. Window 1m, factor 1440.
    // Per day 1_440_000_000 bytes = 1.440 GB. Per month × 30 = 43_200_000_000 = 43.200 GB.
    // Ingest 0.10: day 14.4 cents -> 0.14. Month 432 cents exactly -> 4.32.
    // × 31 days: 44_640_000_000 bytes = 44.640 GB. Month 446.4 cents -> 4.46. Day stays 0.14.
    // Index 0.20 is double the rate, not double the rounded amount:
    // day 28.8 cents -> 0.29. Month 864 cents exactly -> 8.64.
    let mut raw = vec![b'q'; 999_999];
    raw.push(b'\n');
    let log = tmp("json-cost.log", &raw);
    let prices = tmp(
        "json-cost.json",
        br#"{"currency":"EUR","source":"kladblok","date":"1999-01-02","ingest_per_gb":0.10}"#,
    );
    let with_index = tmp(
        "json-cost-index.json",
        br#"{"currency":"EUR","source":"kladblok","date":"1999-01-02","ingest_per_gb":0.10,"index_per_gb":0.20}"#,
    );
    let file_path = log.to_str().expect("path");
    let price = prices.to_str().expect("path");

    let (code, price_only, err) = run_exit(&["scan", "--json", "--prices", price, file_path]);
    assert_eq!(code, 3);
    assert!(price_only.is_empty(), "{price_only}");
    assert_eq!(err, "--prices only works together with --window\n");

    let (code, window_only, err) = run_exit(&["scan", "--json", "--window", "1m", file_path]);
    assert_eq!(code, 0);
    assert!(err.is_empty(), "{err}");
    let window_only = json(&window_only);
    assert_eq!(key_list(&window_only), set(SCAN_BASIS));
    assert!(window_only["estimate"].is_null());
    assert_eq!(window_only["window"]["seconds"], 60);
    assert_eq!(window_only["window"]["factor"], "1440");
    assert_eq!(window_only["totals"]["per_day"]["bytes"], 1_440_000_000);
    assert_eq!(window_only["totals"]["per_day"]["gb"], "1.440");
    assert_eq!(
        window_only["totals"]["per_month"]["bytes"],
        43_200_000_000u64
    );
    assert_eq!(window_only["totals"]["per_month"]["gb"], "43.200");
    assert!(!has_text(&window_only, "estimate:"));

    let (code, day30, err) = run_exit(&[
        "scan", "--json", "--window", "1m", "--prices", price, file_path,
    ]);
    assert_eq!(code, 0);
    assert!(err.is_empty(), "{err}");
    valid_via_json_tool(&day30);
    let day30 = json(&day30);
    let mut with_costs = set(SCAN_BASIS);
    with_costs.insert("costs");
    assert_eq!(key_list(&day30), with_costs);
    assert_eq!(day30["estimate"], ESTIMATE);
    assert_eq!(day30["costs"]["currency"], "EUR");
    assert_eq!(day30["costs"]["source"], "kladblok");
    assert_eq!(day30["costs"]["date"], "1999-01-02");
    assert_eq!(day30["costs"]["gb_bytes"], 1_000_000_000);
    assert!(day30["costs"]["totals"].get("index").is_none());
    assert_eq!(day30["costs"]["totals"]["ingest"]["per_day"], "0.14");
    assert_eq!(day30["costs"]["totals"]["ingest"]["per_month"], "4.32");
    assert_eq!(day30["costs"]["patterns"][0]["ingest"]["per_day"], "0.14");
    assert_eq!(day30["costs"]["patterns"][0]["ingest"]["per_month"], "4.32");
    assert!(day30["costs"].get("savings").is_none());
    assert!(day30["patterns"][0].get("masks").is_none());
    assert_eq!(day30["patterns"][0]["text"], "<VAR>");
    assert!(!has_text(&day30, "q"));

    let (code, day31, err) = run_exit(&[
        "scan",
        "--json",
        "--window",
        "1m",
        "--days-per-month",
        "31",
        "--prices",
        price,
        file_path,
    ]);
    assert_eq!(code, 0);
    assert!(err.is_empty(), "{err}");
    let day31 = json(&day31);
    assert_eq!(day31["inputs"]["days_per_month"], 31);
    assert_eq!(day31["totals"]["per_day"]["gb"], "1.440");
    assert_eq!(day31["totals"]["per_month"]["bytes"], 44_640_000_000u64);
    assert_eq!(day31["totals"]["per_month"]["gb"], "44.640");
    assert_eq!(day31["costs"]["totals"]["ingest"]["per_day"], "0.14");
    assert_eq!(day31["costs"]["totals"]["ingest"]["per_month"], "4.46");

    let (code, index, err) = run_exit(&[
        "scan",
        "--json",
        "--window",
        "1m",
        "--prices",
        with_index.to_str().expect("path"),
        file_path,
    ]);
    for file_path in [&log, &prices, &with_index] {
        let _ = std::fs::remove_file(file_path);
    }
    assert_eq!(code, 0);
    assert!(err.is_empty(), "{err}");
    let index = json(&index);
    assert_eq!(index["costs"]["totals"]["index"]["per_day"], "0.29");
    assert_eq!(index["costs"]["totals"]["index"]["per_month"], "8.64");
    assert_eq!(index["costs"]["totals"]["ingest"]["per_day"], "0.14");
}

#[test]
fn explain_is_in_the_json_only_with_explain() {
    // Same 1_000_000 bytes. The step is the fraction before rounding: 0.144 -> 0.14.
    let mut raw = vec![b'q'; 999_999];
    raw.push(b'\n');
    let log = tmp("json-explain.log", &raw);
    let prices = tmp(
        "json-explain.json",
        br#"{"currency":"EUR","source":"kladblok","date":"1999-01-02","ingest_per_gb":0.10}"#,
    );
    let file_path = log.to_str().expect("path");
    let price = prices.to_str().expect("path");
    let steps =
        "1000000 × 1440 / 1000000000 × (1/10) = 0.144 → 0.14 per day, × 30 = 4.32 → 4.32 per month";

    let (code, without, err) = run_exit(&[
        "scan", "--json", "--window", "1m", "--prices", price, file_path,
    ]);
    let (code_with, with, err_with) = run_exit(&[
        "scan",
        "--json",
        "--explain",
        "--window",
        "1m",
        "--prices",
        price,
        file_path,
    ]);
    let _ = std::fs::remove_file(&log);
    let _ = std::fs::remove_file(&prices);
    assert_eq!(code, 0);
    assert!(err.is_empty(), "{err}");
    assert!(!without.contains("0.144"), "{without}");
    assert!(!without.contains("steps"), "{without}");

    assert_eq!(code_with, 0);
    assert!(err_with.is_empty(), "{err_with}");
    valid_via_json_tool(&with);
    let with = json(&with);
    assert_eq!(with["costs"]["totals"]["ingest"]["steps"], steps);
    assert_eq!(with["costs"]["patterns"][0]["ingest"]["steps"], steps);
    assert_eq!(with["patterns"][0]["masks"]["var"], 1);
    assert_eq!(with["patterns"][0]["masks"]["num"], 0);
    assert_eq!(with["patterns"][0]["masks"]["ts"], 0);
    assert_eq!(key_list(&with), {
        let mut keys = set(SCAN_BASIS);
        keys.insert("costs");
        keys
    });
}

#[test]
fn rules_only_with_rules_and_rare_sample() {
    // 4 × the dropped line = 16 bytes, 6 × the kept line = 36. Total 10 lines, 52 bytes.
    // Drop keeps 6 lines and 36 bytes. Savings 4 lines, 16 bytes.
    let mut raw = Vec::new();
    for _ in 0..4 {
        raw.extend_from_slice(b"weg\n");
    }
    for _ in 0..6 {
        raw.extend_from_slice(b"blijf\n");
    }
    let log = tmp("json-filter.log", &raw);
    let lines = tmp("json-filter.rules", b"drop ^weg$\n");
    let file_path = log.to_str().expect("path");
    let (code, without, err) = run_exit(&["scan", "--json", file_path]);
    assert_eq!(code, 0);
    assert!(err.is_empty(), "{err}");
    assert_eq!(key_list(&json(&without)), set(SCAN_BASIS));

    let (code, with, err) = run_exit(&[
        "scan",
        "--json",
        "--rules",
        lines.to_str().expect("path"),
        file_path,
    ]);
    let _ = std::fs::remove_file(&log);
    let _ = std::fs::remove_file(&lines);
    assert_eq!(code, 0);
    assert!(err.is_empty(), "{err}");
    valid_via_json_tool(&with);
    assert!(!with.contains("^weg$"), "{with}");
    assert!(!with.contains("drop"), "{with}");
    let with = json(&with);
    let mut keys = set(SCAN_BASIS);
    keys.insert("rules");
    assert_eq!(key_list(&with), keys);
    assert_eq!(with["rules"]["applied"], 1);
    assert_eq!(with["rules"]["old"]["lines"], 10);
    assert_eq!(with["rules"]["old"]["bytes"], 52);
    assert_eq!(with["rules"]["new"]["lines"], 6);
    assert_eq!(with["rules"]["new"]["bytes"], 36);
    assert_eq!(with["rules"]["saved"]["lines"], 4);
    assert_eq!(with["rules"]["saved"]["bytes"], 16);
    let gone = with["rules"]["patterns"]
        .as_array()
        .expect("patterns")
        .iter()
        .find(|item| item["text"] == "weg")
        .expect("gone");
    assert_eq!(gone["old_lines"], 4);
    assert_eq!(gone["new_lines"], 0);
    assert_eq!(gone["saved_bytes"], 16);
    assert_eq!(gone["rule_lines"][0], 1);

    // One line of 9 a's plus `\n` = 10 bytes. sample 0.1: floor(1 × 0.1) = 0 lines
    // and floor(10 × 0.1) = 1 byte. The byte stays, even though the line count is 0.
    let one_apostrophe = tmp("json-nine-a.log", b"aaaaaaaaa\n");
    let sample = tmp("json-nine-a.rules", b"sample 0.1 ^a{9}$\n");
    let (code, output_2, err) = run_exit(&[
        "scan",
        "--json",
        "--rules",
        sample.to_str().expect("path"),
        one_apostrophe.to_str().expect("path"),
    ]);
    let _ = std::fs::remove_file(&one_apostrophe);
    let _ = std::fs::remove_file(&sample);
    assert_eq!(code, 0);
    assert!(err.is_empty(), "{err}");
    assert!(!output_2.contains("a{9}"), "{output_2}");
    assert!(!output_2.contains("0.1"), "{output_2}");
    assert!(!output_2.contains("sample"), "{output_2}");
    let output_2 = json(&output_2);
    assert_eq!(output_2["rules"]["new"]["lines"], 0);
    assert_eq!(output_2["rules"]["new"]["bytes"], 1);
    assert_eq!(output_2["rules"]["saved"]["lines"], 1);
    assert_eq!(output_2["rules"]["saved"]["bytes"], 9);
    assert_eq!(output_2["rules"]["patterns"][0]["new_lines"], 0);
    assert_eq!(output_2["rules"]["patterns"][0]["new_bytes"], 1);

    // An empty rules file counts as 0 commands. Nothing is dropped.
    let empty_log = tmp("json-empty-rules.log", b"aa\n");
    let empty = tmp("json-empty.rules", b"\n");
    let (code, output_2, err) = run_exit(&[
        "scan",
        "--json",
        "--rules",
        empty.to_str().expect("path"),
        empty_log.to_str().expect("path"),
    ]);
    let _ = std::fs::remove_file(&empty_log);
    let _ = std::fs::remove_file(&empty);
    assert_eq!(code, 0);
    assert!(err.is_empty(), "{err}");
    let output_2 = json(&output_2);
    assert_eq!(output_2["rules"]["applied"], 0);
    assert_eq!(output_2["rules"]["old"]["bytes"], 3);
    assert_eq!(output_2["rules"]["new"]["bytes"], 3);
    assert_eq!(output_2["rules"]["saved"]["bytes"], 0);
}

#[test]
fn diff_costs_signs_and_rules() {
    // Delta -5 bytes. Window 1d, gb_bytes 1000, 30 days.
    // Month cents = bytes × days × price × 100 / 1000.
    // Price 1: 15 cents -> -0.15. Price 2: 30 cents -> -0.30.
    let old = manifest("tests/fixtures/diff-old.log");
    let new_sample = manifest("tests/fixtures/diff-new.log");
    let prices = tmp(
        "json-diff-price.json",
        br#"{"currency":"EUR","source":"kladblok","date":"1999-01-02","ingest_per_gb":1,"index_per_gb":2,"gb_bytes":1000}"#,
    );
    let old_path = old.to_str().expect("path");
    let new_path = new_sample.to_str().expect("path");
    let price_path = prices.to_str().expect("path");

    let (code, without, err) =
        run_exit(&["diff", "--json", "--prices", price_path, old_path, new_path]);
    assert_eq!(code, 3);
    assert!(without.is_empty(), "{without}");
    assert_eq!(err, "--prices only works together with --window\n");

    let (code, with, err) = run_exit(&[
        "diff", "--json", "--window", "1d", "--prices", price_path, old_path, new_path,
    ]);
    let _ = std::fs::remove_file(&prices);
    assert_eq!(code, 0);
    assert!(err.is_empty(), "{err}");
    valid_via_json_tool(&with);
    let with = json(&with);
    let mut keys = set(DIFF_BASIS);
    keys.insert("costs");
    assert_eq!(key_list(&with), keys);
    assert_eq!(with["estimate"], ESTIMATE);
    assert_eq!(with["costs"]["month"]["bytes_delta"], -5);
    assert_eq!(with["costs"]["month"]["ingest"], "-0.15");
    assert_eq!(with["costs"]["month"]["index"], "-0.30");
    assert_eq!(with["window"]["factor"], "1");
    assert_eq!(with["window"]["seconds"], 86_400);

    // aa\n = 3, bb\n = 3. Old 3 bytes, new 6, delta +3.
    // 3 × 30 × 100 / 1000 = 9 cents -> +0.09.
    let plus_old = tmp("json-plus-old.log", b"aa\n");
    let plus_new = tmp("json-plus-new.log", b"aa\nbb\n");
    let plus_price = tmp(
        "json-plus.json",
        br#"{"currency":"EUR","source":"kladblok","date":"1999-01-02","ingest_per_gb":1,"gb_bytes":1000}"#,
    );
    let (code, plus, err) = run_exit(&[
        "diff",
        "--json",
        "--window",
        "1d",
        "--prices",
        plus_price.to_str().expect("path"),
        plus_old.to_str().expect("path"),
        plus_new.to_str().expect("path"),
    ]);
    for file_path in [&plus_old, &plus_new, &plus_price] {
        let _ = std::fs::remove_file(file_path);
    }
    assert_eq!(code, 0);
    assert!(err.is_empty(), "{err}");
    let plus = json(&plus);
    assert_eq!(plus["totals_delta"]["bytes"], 3);
    assert_eq!(plus["totals_delta"]["lines"], 1);
    assert_eq!(plus["costs"]["month"]["ingest"], "+0.09");
    assert!(plus["costs"]["month"].get("index").is_none());
    assert_eq!(plus["added"][0]["text"], "bb");
    assert_eq!(plus["totals"]["added_share"], "50.0");

    // `drop b.ta` removes beta before the comparison. Old becomes 12 bytes, equal to new
    // in total. The regex itself does not appear in the output.
    let lines = tmp("json-diff.rules", b"drop b.ta\n");
    let (code, with_rules, err) = run_exit(&[
        "diff",
        "--json",
        "--rules",
        lines.to_str().expect("path"),
        old_path,
        new_path,
    ]);
    let _ = std::fs::remove_file(&lines);
    assert_eq!(code, 0);
    assert!(err.is_empty(), "{err}");
    assert!(!with_rules.contains("b.ta"), "{with_rules}");
    assert!(!with_rules.contains("drop"), "{with_rules}");
    let with_rules = json(&with_rules);
    let mut keys = set(DIFF_BASIS);
    keys.insert("rules");
    assert_eq!(key_list(&with_rules), keys);
    assert_eq!(with_rules["rules"]["applied"], 1);
    assert_eq!(with_rules["totals"]["old"]["bytes"], 12);
    assert_eq!(with_rules["totals"]["old"]["lines"], 2);
    assert!(with_rules["removed"]
        .as_array()
        .expect("removed")
        .is_empty());
    assert_eq!(with_rules["added"][0]["text"], "gamma");
}

fn diff_refuses(args: &[&str], message: &str) {
    let (code, output_2, err) = run_exit(args);
    assert_eq!(code, 3, "{args:?} stderr: {err}");
    assert!(output_2.is_empty(), "{args:?}: {output_2}");
    assert_eq!(err, message, "{args:?}");
}

#[test]
fn diff_explain_works_only_on_scan() {
    // `--explain` does not apply to diff.
    diff_refuses(
        &["diff", "--explain", "OLD", "NEW"],
        "--explain only works with scan\n",
    );
    diff_refuses(
        &["diff", "--json", "--explain", "OLD", "NEW"],
        "--explain only works with scan\n",
    );
}

#[test]
fn diff_top_works_only_on_scan() {
    // `--top` does not apply to diff.
    diff_refuses(
        &["diff", "--top", "1", "OLD", "NEW"],
        "--top only works with scan\n",
    );
    diff_refuses(
        &["diff", "--json", "--top", "1", "OLD", "NEW"],
        "--top only works with scan\n",
    );
}

#[test]
fn diff_budget_month_applies_only_to_scan() {
    // `diff --budget-month 1 OLD NEW` stops before reading the files.
    diff_refuses(
        &["diff", "--budget-month", "1", "OLD", "NEW"],
        "the monthly budget only applies to scan\n",
    );
    diff_refuses(
        &["diff", "--json", "--budget-month", "1", "OLD", "NEW"],
        "the monthly budget only applies to scan\n",
    );
}

#[test]
fn zero_difference_empty_input_and_top() {
    // Two equal samples: changed with delta 0, added and removed empty.
    let one_apostrophe = tmp("json-equal.log", b"aa\n");
    let file_path = one_apostrophe.to_str().expect("path");
    let (code, output_2, err) = run_exit(&["diff", "--json", file_path, file_path]);
    assert_eq!(code, 0);
    assert!(err.is_empty(), "{err}");
    let output_2 = json(&output_2);
    assert_eq!(output_2["totals_delta"]["bytes"], 0);
    assert_eq!(output_2["totals_delta"]["lines"], 0);
    assert!(output_2["added"].as_array().expect("added").is_empty());
    assert!(output_2["removed"].as_array().expect("removed").is_empty());
    assert_eq!(output_2["changed"][0]["bytes"]["delta"], 0);
    assert_eq!(output_2["changed"][0]["share"]["delta"], "0.0");
    assert_eq!(output_2["totals"]["added_bytes"], 0);
    assert_eq!(output_2["totals"]["added_share"], "0.0");

    let prices = tmp(
        "json-nul.json",
        br#"{"currency":"EUR","source":"kladblok","date":"1999-01-02","ingest_per_gb":1,"gb_bytes":1000}"#,
    );
    let (code, geld, err) = run_exit(&[
        "diff",
        "--json",
        "--window",
        "1d",
        "--prices",
        prices.to_str().expect("path"),
        file_path,
        file_path,
    ]);
    let _ = std::fs::remove_file(&one_apostrophe);
    let _ = std::fs::remove_file(&prices);
    assert_eq!(code, 0);
    assert!(err.is_empty(), "{err}");
    assert_eq!(json(&geld)["costs"]["month"]["ingest"], "0.00");

    // Two empty files: no division by zero.
    let empty_a = tmp("json-empty-a.log", b"");
    let empty_b = tmp("json-empty-b.log", b"\n\n");
    let (code, empty, err) = run_exit(&[
        "diff",
        "--json",
        empty_a.to_str().expect("path"),
        empty_b.to_str().expect("path"),
    ]);
    let _ = std::fs::remove_file(&empty_a);
    let _ = std::fs::remove_file(&empty_b);
    assert_eq!(code, 0);
    assert!(err.is_empty(), "{err}");
    valid_via_json_tool(&empty);
    let empty = json(&empty);
    assert_eq!(empty["totals"]["old"]["bytes"], 0);
    assert_eq!(empty["totals"]["new"]["lines"], 0);
    assert_eq!(empty["totals"]["added_share"], "0.0");
    assert!(empty["patterns"].as_array().expect("patterns").is_empty());

    // Last line without a newline: `xy` counts as 3. Share 100.0%.
    let xy = tmp("json-xy.log", b"xy");
    let (code, output_2, err) = run_exit(&["scan", "--json", xy.to_str().expect("path")]);
    let _ = std::fs::remove_file(&xy);
    assert_eq!(code, 0);
    assert!(err.is_empty(), "{err}");
    let output_2 = json(&output_2);
    assert_eq!(output_2["totals"]["lines"], 1);
    assert_eq!(output_2["totals"]["bytes"], 3);
    assert_eq!(output_2["patterns"][0]["text"], "xy");
    assert_eq!(output_2["patterns"][0]["share"], "100.0");

    // ccc\n = 4, bb\n = 3, a\n = 2. Top 1 keeps ccc; the rest is 5 bytes, 2 lines.
    // 4/9 -> 444 tenths = 44.4%. 5/9 -> 555 remainder 5, 2*5 >= 9, so 55.6%.
    let top = tmp("json-top.log", b"a\nbb\nccc\n");
    let (code, output_2, err) =
        run_exit(&["scan", "--json", "--top", "1", top.to_str().expect("path")]);
    let _ = std::fs::remove_file(&top);
    assert_eq!(code, 0);
    assert!(err.is_empty(), "{err}");
    let output_2 = json(&output_2);
    assert_eq!(output_2["inputs"]["top"], 1);
    assert_eq!(output_2["totals"]["bytes"], 9);
    assert_eq!(output_2["patterns"][0]["text"], "ccc");
    assert_eq!(output_2["patterns"][0]["bytes"], 4);
    assert_eq!(output_2["patterns"][0]["share"], "44.4");
    assert_eq!(output_2["patterns"][0]["rest"], false);
    assert_eq!(output_2["patterns"][1]["id"], "other");
    assert_eq!(output_2["patterns"][1]["rest"], true);
    assert_eq!(output_2["patterns"][1]["lines"], 2);
    assert_eq!(output_2["patterns"][1]["bytes"], 5);
    assert_eq!(output_2["patterns"][1]["share"], "55.6");

    // 0 days: the month is 0 bytes. The day stays the sample × factor.
    // `z\n` = 2 bytes, window 1d, factor 1. Month × 0 = 0.
    let day0 = tmp("json-day0.log", b"z\n");
    let (code, output_2, err) = run_exit(&[
        "scan",
        "--json",
        "--window",
        "1d",
        "--days-per-month",
        "0",
        day0.to_str().expect("path"),
    ]);
    let _ = std::fs::remove_file(&day0);
    assert_eq!(code, 0);
    assert!(err.is_empty(), "{err}");
    let output_2 = json(&output_2);
    assert_eq!(output_2["inputs"]["days_per_month"], 0);
    assert_eq!(output_2["totals"]["per_day"]["bytes"], 2);
    assert_eq!(output_2["totals"]["per_month"]["bytes"], 0);
    assert_eq!(output_2["totals"]["per_month"]["gb"], "0.000");
}

#[test]
fn json_escapes_special_characters_and_keeps_the_message() {
    // `say "a\b"` is 9 characters. With `\n` the line counts as 10. The quote and
    // backslash stay in the pattern and must be escaped in JSON.
    let quote = tmp("json-quote.log", b"say \"a\\b\"\n");
    let (code, output_2, err) = run_exit(&["scan", "--json", quote.to_str().expect("path")]);
    let _ = std::fs::remove_file(&quote);
    assert_eq!(code, 0);
    assert!(err.is_empty(), "{err}");
    valid_via_json_tool(&output_2);
    assert!(!output_2.as_bytes().contains(&b'\r'));
    let output_2 = json(&output_2);
    assert_eq!(output_2["totals"]["bytes"], 10);
    assert_eq!(output_2["patterns"][0]["text"], "say \"a\\b\"");

    // A CR in the middle of the line counts (4 bytes) and is not raw in the output.
    let cr = tmp("json-cr.log", b"a\rb\n");
    let (code, output_2, err) = run_exit(&["scan", "--json", cr.to_str().expect("path")]);
    let _ = std::fs::remove_file(&cr);
    assert_eq!(code, 0);
    assert!(err.is_empty(), "{err}");
    assert!(!output_2.as_bytes().contains(&b'\r'), "{output_2:?}");
    valid_via_json_tool(&output_2);
    let output_2 = json(&output_2);
    assert_eq!(output_2["totals"]["bytes"], 4);
    assert_eq!(output_2["patterns"][0]["text"], "a\rb");

    // JSON lines: the bytes are the whole line (23 + 1). The message keeps the newline.
    let raw = b"{\"message\":\"een\\ntwee\"}\n";
    assert_eq!(raw.len(), 24);
    let jsonl = tmp("json-newline.jsonl", raw);
    let (code, output_2, err) = run_exit(&[
        "scan",
        "--json",
        "--format",
        "jsonl",
        "--message-field",
        "note",
        jsonl.to_str().expect("path"),
    ]);
    // This file has no `note`. The pattern is therefore `<NO-MESSAGE>`.
    assert_eq!(code, 0);
    assert!(err.is_empty(), "{err}");
    let without_field = json(&output_2);
    assert_eq!(without_field["inputs"]["format"], "jsonl");
    assert_eq!(without_field["inputs"]["message_field"], "note");
    assert_eq!(without_field["patterns"][0]["text"], "<NO-MESSAGE>");
    assert_eq!(without_field["totals"]["bytes"], 24);

    let (code, output_2, err) = run_exit(&[
        "scan",
        "--json",
        "--format",
        "jsonl",
        jsonl.to_str().expect("path"),
    ]);
    let _ = std::fs::remove_file(&jsonl);
    assert_eq!(code, 0);
    assert!(err.is_empty(), "{err}");
    valid_via_json_tool(&output_2);
    let output_2 = json(&output_2);
    assert!(output_2["inputs"]["message_field"].is_null());
    assert_eq!(output_2["patterns"][0]["text"], "een\ntwee");
    assert_eq!(output_2["totals"]["bytes"], 24);
}

#[test]
fn sample_with_zero_lines_stays_in_the_diff() {
    // 9 a's plus `\n` = 10 bytes. After sample 0.1: 0 lines and 1 byte.
    // The new sample is empty. The pattern is gone, with old bytes 1.
    let old = tmp("json-fraction-old.log", b"aaaaaaaaa\n");
    let new_sample = tmp("json-fraction-new.log", b"");
    let lines = tmp("json-fraction.rules", b"sample 0.1 ^a{9}$\n");
    let (code, output_2, err) = run_exit(&[
        "diff",
        "--json",
        "--rules",
        lines.to_str().expect("path"),
        old.to_str().expect("path"),
        new_sample.to_str().expect("path"),
    ]);
    for file_path in [&old, &new_sample, &lines] {
        let _ = std::fs::remove_file(file_path);
    }
    assert_eq!(code, 0);
    assert!(err.is_empty(), "{err}");
    assert!(!output_2.contains("a{9}"), "{output_2}");
    let output_2 = json(&output_2);
    assert_eq!(output_2["rules"]["applied"], 1);
    assert_eq!(output_2["totals"]["old"]["lines"], 0);
    assert_eq!(output_2["totals"]["old"]["bytes"], 1);
    assert_eq!(output_2["totals"]["new"]["bytes"], 0);
    assert_eq!(output_2["totals_delta"]["bytes"], -1);
    assert_eq!(output_2["removed"][0]["lines"]["old"], 0);
    assert_eq!(output_2["removed"][0]["bytes"]["old"], 1);
    assert_eq!(output_2["removed"][0]["share"]["old"], "100.0");
}

#[test]
fn budget_and_errors_keep_the_same_codes() {
    let log = tmp("json-budget.log", b"z\n");
    let prices = tmp(
        "json-budget.json",
        br#"{"currency":"EUR","source":"kladblok","date":"1999-01-02","ingest_per_gb":1,"gb_bytes":1}"#,
    );
    let (code, output_2, err) = run_exit(&[
        "scan",
        "--json",
        "--window",
        "1d",
        "--prices",
        prices.to_str().expect("path"),
        "--budget-month",
        "0",
        log.to_str().expect("path"),
    ]);
    let _ = std::fs::remove_file(&log);
    let _ = std::fs::remove_file(&prices);
    assert_eq!(code, 1);
    assert_eq!(err, "monthly cost above budget\n");
    valid_via_json_tool(&output_2);
    assert_eq!(json(&output_2)["schema_version"], 1);
    assert_eq!(json(&output_2)["estimate"], ESTIMATE);

    let (code, output_2, err) = run_exit(&["scan", "--json", "--budget-month", "1", "x.log"]);
    assert_eq!(code, 3);
    assert!(output_2.is_empty(), "{output_2}");
    assert_eq!(err, "budget requires a window\n");

    let (code, output_2, err) = run_exit(&[
        "diff",
        "--json",
        "--max-new-share",
        "49",
        "tests/fixtures/diff-old.log",
        "tests/fixtures/diff-new.log",
    ]);
    assert_eq!(code, 1);
    assert_eq!(err, "new patterns above the threshold\n");
    assert_eq!(json(&output_2)["totals"]["added_share"], "50.0");

    let broken = tmp("json-broken.jsonl", b"{NOT-JSON-0001\n");
    let (code, output_2, err) = run_exit(&[
        "scan",
        "--json",
        "--format",
        "jsonl",
        broken.to_str().expect("path"),
    ]);
    let _ = std::fs::remove_file(&broken);
    assert_eq!(code, 2);
    assert!(output_2.is_empty(), "{output_2}");
    assert_eq!(err, "invalid JSON on line 1\n");
    assert!(!err.contains("NOT-JSON-0001"));

    // 10^19 seconds × 10^20 gb_bytes does not fit in u128. No JSON fragment.
    let over_log = tmp("json-over.log", b"ok\n");
    let price_over = tmp(
        "json-over.json",
        br#"{"currency":"EUR","source":"kladblok","date":"1999-01-02","ingest_per_gb":1,"gb_bytes":1e20}"#,
    );
    for cmd in [
        vec![
            "scan",
            "--json",
            "--window",
            "10000000000000000000s",
            "--prices",
            price_over.to_str().expect("path"),
            over_log.to_str().expect("path"),
        ],
        vec![
            "diff",
            "--json",
            "--window",
            "10000000000000000000s",
            "--prices",
            price_over.to_str().expect("path"),
            over_log.to_str().expect("path"),
            over_log.to_str().expect("path"),
        ],
    ] {
        let (code, output_2, err) = run_exit(&cmd);
        assert_eq!(code, 3, "{cmd:?}");
        assert!(output_2.is_empty(), "{cmd:?}: {output_2}");
        assert_eq!(err, "calculation overflow\n", "{cmd:?}");
        assert!(!err.contains("ok"), "{cmd:?}");
    }
    let _ = std::fs::remove_file(&over_log);
    let _ = std::fs::remove_file(&price_over);
}
