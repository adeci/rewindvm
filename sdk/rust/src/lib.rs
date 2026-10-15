// SPDX-License-Identifier: MIT
//! The Rewind SDK for Rust: assertions and test marks a program writes to
//! /dev/rewind, which `rewind check` judges on every run it tries. Outside
//! Rewind there is no /dev/rewind and every call does nothing, so the
//! calls can stay in production code. sdk/README.md says what each one
//! means.
//!
//! ```
//! # let (total, deposited, pending) = (200, 200, vec![1, 2]);
//! rewind_sdk::always!(total == deposited, "ledger-balances", "total={total} deposited={deposited}");
//! rewind_sdk::sometimes!(pending.len() > 1, "settled-with-overlap");
//! ```
//!
//! An id has no spaces. A message is formatted only when it is written.
//! Each id writes a pass or a hit once, and a failure or a miss once, per
//! process, since every write costs the VM an exit.

use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::{Mutex, OnceLock};

/// The device the lines go to.
const DEVICE: &str = "/dev/rewind";

/// What every line starts with: the SDK's word and the protocol version.
const PREFIX: &str = "rewind-sdk 1";

/// The device keeps a write of up to 4096 bytes as one mark.
const LINE_MAX: usize = 4000;

/// How a test ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestResult {
    Pass,
    Fail,
    Skip,
}

/// The device, opened once per process; None outside Rewind.
fn device() -> Option<&'static File> {
    static DEVICE_FILE: OnceLock<Option<File>> = OnceLock::new();
    DEVICE_FILE
        .get_or_init(|| OpenOptions::new().write(true).open(DEVICE).ok())
        .as_ref()
}

/// The lines written once already, so they are not written again.
static SENT: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// Writes `words` as one line, in one write.
fn say(words: &str) {
    let Some(mut file) = device() else {
        return;
    };
    let mut line = format!("{PREFIX} {}", words.replace('\n', " "));
    if line.len() > LINE_MAX {
        let mut cut = LINE_MAX;
        while !line.is_char_boundary(cut) {
            cut -= 1;
        }
        line.truncate(cut);
    }
    line.push('\n');
    let _ = file.write_all(line.as_bytes());
}

/// Writes `key`'s line the first time only.
fn say_once(key: String, words: impl FnOnce() -> String) {
    if device().is_none() {
        return;
    }
    let first = SENT
        .lock()
        .map(|mut sent| sent.insert(key))
        .unwrap_or(false);
    if first {
        say(&words());
    }
}

/// `words` followed by `message` when there is one.
fn with(words: String, message: String) -> String {
    if message.is_empty() {
        words
    } else {
        format!("{words} {message}")
    }
}

/// `cond` holds every time the program gets here. Returns `cond`.
pub fn always(cond: bool, id: &str, message: impl FnOnce() -> String) -> bool {
    if cond {
        say_once(format!("always pass {id}"), || format!("always pass {id}"));
    } else {
        say_once(format!("always fail {id}"), || {
            with(format!("always fail {id}"), message())
        });
    }
    cond
}

/// Some run should make `cond` true here. Returns `cond`.
pub fn sometimes(cond: bool, id: &str) -> bool {
    let words = if cond {
        format!("sometimes hit {id}")
    } else {
        format!("sometimes miss {id}")
    };
    say_once(words.clone(), || words);
    cond
}

/// No run should get here.
pub fn unreachable(id: &str, message: impl FnOnce() -> String) {
    say_once(format!("unreachable {id}"), || {
        with(format!("unreachable {id}"), message())
    });
}

/// A schedule point: an exit where `rewind check` can switch to another
/// thread or hold this one up, with nothing written to the run's trace. Put
/// one where a switch matters but the code makes no system call, such as
/// between an atomic load and the store computed from it.
pub fn point() {
    if let Some(mut file) = device() {
        let _ = file.write(&[]);
    }
}

/// A test named `name`, which may have spaces, starts.
pub fn test_begin(name: &str) {
    say(&format!("test-begin {name}"));
}

/// The test named `name` ended as `result` says.
pub fn test_end(name: &str, result: TestResult) {
    let word = match result {
        TestResult::Pass => "pass",
        TestResult::Fail => "fail",
        TestResult::Skip => "skip",
    };
    say(&format!("test-end {word} {name}"));
}

/// `always!(cond, "id")` or `always!(cond, "id", "format", args...)`.
#[macro_export]
macro_rules! always {
    ($cond:expr, $id:expr $(,)?) => {
        $crate::always($cond, $id, ::std::string::String::new)
    };
    ($cond:expr, $id:expr, $($message:tt)+) => {
        $crate::always($cond, $id, || ::std::format!($($message)+))
    };
}

/// `sometimes!(cond, "id")`.
#[macro_export]
macro_rules! sometimes {
    ($cond:expr, $id:expr $(,)?) => {
        $crate::sometimes($cond, $id)
    };
}

/// `unreachable!("id")` or `unreachable!("id", "format", args...)`, called
/// by its path, `rewind_sdk::unreachable!`, beside the standard one.
#[macro_export]
macro_rules! unreachable {
    ($id:expr $(,)?) => {
        $crate::unreachable($id, ::std::string::String::new)
    };
    ($id:expr, $($message:tt)+) => {
        $crate::unreachable($id, || ::std::format!($($message)+))
    };
}
