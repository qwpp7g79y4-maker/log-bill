//! SIGINT stops before a report is published.
//!
//! The signal goes only to the child this test spawned, and only through the
//! pidfd opened on that child. Flags are 0, so the signal is not sent to the
//! process group. The test does not call `kill` with a pid.

#![cfg(target_os = "linux")]

use std::fs;
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Running {
    child: Child,
    pidfd: OwnedFd,
    stdin: Option<std::process::ChildStdin>,
}

impl Drop for Running {
    fn drop(&mut self) {
        // Closing stdin lets a child blocked in `read` wake if it is still
        // alive. The following signal, if any, still uses this child's pidfd.
        self.stdin.take();
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = send(&self.pidfd, libc::SIGKILL);
            let _ = self.child.wait();
        }
    }
}

impl Running {
    fn spawn(output: &std::path::Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_log-bill"))
            .args([
                "scan",
                "--output",
                output.to_str().expect("utf-8 path"),
                "-",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn log-bill");
        let stdin = child.stdin.take();
        let pid = child.id();
        assert_ne!(pid, std::process::id(), "refusing to signal this test");
        let pidfd = open_pidfd(pid).expect("pidfd_open on the spawned child");
        assert_eq!(
            pid_of(&pidfd),
            Some(pid),
            "pidfd does not refer to the spawned child"
        );
        Self {
            child,
            pidfd,
            stdin,
        }
    }

    fn wait_until_blocked(&mut self) -> bool {
        let pid = self.child.id();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut streak = 0u32;
        while Instant::now() < deadline {
            if self.child.try_wait().expect("try_wait").is_some() {
                return false;
            }
            if blocked_in_read(pid) {
                streak += 1;
                if streak >= 5 {
                    return true;
                }
            } else {
                streak = 0;
            }
            std::thread::sleep(Duration::from_millis(40));
        }
        false
    }

    fn interrupt(self) -> (i32, Vec<u8>, Vec<u8>) {
        let mut running = self;
        assert!(
            running.wait_until_blocked(),
            "child was not blocked in read"
        );
        send(&running.pidfd, libc::SIGINT).expect("pidfd_send_signal SIGINT");
        let status = running.wait_exit();
        let code = status.code().unwrap_or_else(|| {
            panic!("child died from a signal instead of exiting 130: {status:?}")
        });
        let (out, err) = running.pipes();
        (code, out, err)
    }

    fn wait_exit(&mut self) -> std::process::ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait().expect("try_wait") {
                return status;
            }
            if Instant::now() >= deadline {
                panic!("child did not exit after SIGINT");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn pipes(&mut self) -> (Vec<u8>, Vec<u8>) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        if let Some(mut stdout) = self.child.stdout.take() {
            stdout.read_to_end(&mut out).expect("stdout");
        }
        if let Some(mut stderr) = self.child.stderr.take() {
            stderr.read_to_end(&mut err).expect("stderr");
        }
        (out, err)
    }
}

/// `flags` is 0: one process, not its process group.
fn send(pidfd: &OwnedFd, signal: i32) -> std::io::Result<()> {
    let rc = unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            pidfd.as_raw_fd() as libc::c_long,
            signal as libc::c_long,
            0 as libc::c_long,
            0 as libc::c_long,
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

fn open_pidfd(pid: u32) -> std::io::Result<OwnedFd> {
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::c_long, 0 as libc::c_long) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { OwnedFd::from_raw_fd(fd as RawFd) })
}

fn pid_of(pidfd: &OwnedFd) -> Option<u32> {
    let text = fs::read_to_string(format!("/proc/self/fdinfo/{}", pidfd.as_raw_fd())).ok()?;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Pid:") {
            return rest.trim().parse().ok();
        }
    }
    None
}

/// `read` on file descriptor 0. A startup read of the binary uses another fd.
fn blocked_in_read(pid: u32) -> bool {
    let Ok(text) = fs::read_to_string(format!("/proc/{pid}/syscall")) else {
        return false;
    };
    let mut fields = text.split_whitespace();
    let Some(number) = fields.next() else {
        return false;
    };
    let Some(fd) = fields.next() else {
        return false;
    };
    number.parse::<i64>().ok() == Some(libc::SYS_read) && (fd == "0x0" || fd == "0")
}

fn stopped(output: &std::path::Path) -> (i32, Vec<u8>, Vec<u8>) {
    Running::spawn(output).interrupt()
}

#[cfg(not(coverage))]
#[test]
fn sigint_writes_no_report_and_exits_130() {
    let root = tempfile::tempdir().expect("scratch dir");
    let report = root.path().join("report.txt");
    assert!(!report.exists());
    let (code, stdout, stderr) = stopped(&report);
    let err = String::from_utf8_lossy(&stderr);
    assert_eq!(code, 130, "stdout {stdout:?} stderr {err}");
    assert_ne!(code, 0);
    assert!(stdout.is_empty(), "{stdout:?}");
    assert!(stderr.is_empty(), "{err}");
    assert!(!report.exists(), "a new report must not be created");
    let names: Vec<_> = fs::read_dir(root.path())
        .expect("list")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
    assert!(names.is_empty(), "no partial file: {names:?}");
}

#[cfg(not(coverage))]
#[test]
fn sigint_leaves_an_existing_output_file_unchanged() {
    let root = tempfile::tempdir().expect("scratch dir");
    let report = root.path().join("report.txt");
    let prior = b"PRIOR-BYTES-NOT-THE-REPORT\n";
    fs::write(&report, prior).unwrap();
    let (code, stdout, stderr) = stopped(&report);
    let err = String::from_utf8_lossy(&stderr);
    assert_eq!(code, 130, "stdout {stdout:?} stderr {err}");
    assert_ne!(code, 0);
    assert!(stdout.is_empty(), "{stdout:?}");
    assert!(stderr.is_empty(), "{err}");
    assert_eq!(fs::read(&report).expect("report"), prior);
    let names: Vec<_> = fs::read_dir(root.path())
        .expect("list")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
    assert_eq!(
        names.len(),
        1,
        "no partial file beside the report: {names:?}"
    );
}
