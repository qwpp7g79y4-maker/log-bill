//! The manual page and the completion scripts follow the real command line.

use std::collections::BTreeSet;
use std::fs;
use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_log-bill"))
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("start log-bill")
}

fn long_options(text: &str) -> BTreeSet<String> {
    text.split_whitespace()
        .filter_map(|word| word.strip_prefix("--"))
        .map(|name| {
            name.chars()
                .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
                .collect::<String>()
        })
        .filter(|name| !name.is_empty())
        .collect()
}

#[test]
fn committed_man_page_is_current() {
    let out = run(&["man"]);
    assert_eq!(out.status.code(), Some(0));
    let committed = fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/man/log-bill.1")).unwrap();
    assert_eq!(
        out.stdout, committed,
        "regenerate: log-bill man > man/log-bill.1"
    );
}

#[test]
fn man_page_names_every_option_of_help() {
    let help = String::from_utf8(run(&["--help"]).stdout).unwrap();
    let man = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/man/log-bill.1"))
        .unwrap()
        .replace("\\-", "-");
    let in_man = long_options(&man.replace("\\fB", " ").replace("\\fR", " "));
    for option in long_options(&help) {
        assert!(in_man.contains(&option), "man page lacks --{option}");
    }
}

#[test]
fn completion_scripts_for_bash_and_zsh() {
    let bash = run(&["completions", "bash"]);
    assert_eq!(bash.status.code(), Some(0));
    let bash_text = String::from_utf8(bash.stdout).unwrap();
    assert!(bash_text.contains("--budget-month"));
    let help_out = run(&["--help"]);
    assert_eq!(help_out.status.code(), Some(0));
    let help = String::from_utf8(help_out.stdout).unwrap();
    let expected = long_options(&help);
    assert!(
        expected.contains("help") && expected.contains("version"),
        "top-level --help should name --help and --version: {expected:?}"
    );
    let in_bash = long_options(&bash_text);
    for option in &expected {
        assert!(in_bash.contains(option), "bash completion lacks --{option}");
    }
    let zsh = run(&["completions", "zsh"]);
    assert_eq!(zsh.status.code(), Some(0));
    assert!(String::from_utf8(zsh.stdout)
        .unwrap()
        .starts_with("#compdef log-bill"));
}

#[test]
fn other_shell_is_a_usage_error() {
    let out = run(&["completions", "fish"]);
    assert_eq!(out.status.code(), Some(3));
    assert!(out.stdout.is_empty());
}

#[test]
fn help_hides_the_generators() {
    let help = String::from_utf8(run(&["--help"]).stdout).unwrap();
    assert!(!help.contains("completions"));
    assert!(!help.contains("  man "));
}
