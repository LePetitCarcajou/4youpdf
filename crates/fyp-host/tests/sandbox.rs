//! The sandbox against hostile modules, written in the WebAssembly text
//! format, and against the real merge module of `plugins/merge`.
//!
//! The merge tests need `plugins/merge/module.wasm`, built by
//! `python tools/build_modules.py`. Without it they are skipped, unless
//! `FYP_REQUIRE_MODULES` is set (CI), which makes its absence a failure.

#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fyp_core::document::Document;
use fyp_core::object::ObjRef;
use fyp_core::ops;
use fyp_host::{discover, Host, HostError, HostLimits, LoadedModule, ParamValue, RunOutput};
use fyp_plugin_api::exchange::Response;
use fyp_plugin_api::Manifest;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(name: &str) -> Vec<u8> {
    let path = root().join("tests/fixtures").join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

const DOCUMENTS: &str = r#"{ kind = "read_document" }, { kind = "write_document" }"#;

fn limits(timeout_ms: u64, memory_mib: u64, max_output_mib: u64) -> String {
    format!(
        "timeout_ms = {timeout_ms}\nmemory_mib = {memory_mib}\nmax_output_mib = {max_output_mib}"
    )
}

/// A test module's manifest: one action `run` taking any number of
/// documents, with the permissions, limits and parameter tables given.
fn manifest(permissions: &str, limits: &str, params: &str) -> Manifest {
    let text = format!(
        r#"
id = "org.example.hostile"
name = "Hostile"
version = "0.0.1"
api_version = "{api}"
license = "AGPL-3.0-or-later"
source = ""
runtime = "wasm"
permissions = [{permissions}]

[limits]
{limits}

[[actions]]
id = "run"
label = "Run"
category = "process"
min_inputs = 0
{params}
"#,
        api = fyp_plugin_api::API_VERSION
    );
    Manifest::from_toml(&text).unwrap()
}

fn default_manifest() -> Manifest {
    manifest(DOCUMENTS, &limits(10_000, 16, 4), "")
}

fn load(manifest: Manifest, wat: &str) -> Result<LoadedModule, HostError> {
    let wasm = wat::parse_str(wat).unwrap_or_else(|e| panic!("{e}\n{wat}"));
    Host::new().unwrap().load_bytes(manifest, &wasm)
}

fn run(module: &LoadedModule, documents: &[&[u8]]) -> Result<RunOutput, HostError> {
    module.run("run", &BTreeMap::new(), documents)
}

const FD_WRITE: &str = r#"(import "wasi_snapshot_preview1" "fd_write" (func $fd_write (param i32 i32 i32 i32) (result i32)))"#;
const PROC_EXIT: &str =
    r#"(import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))"#;

/// A module whose `_start` writes `bytes` on its standard output at once.
fn writing(bytes: &[u8]) -> String {
    let escaped: String = bytes.iter().map(|b| format!("\\{b:02x}")).collect();
    format!(
        r#"(module
  {FD_WRITE}
  (memory (export "memory") {pages})
  (data (i32.const 16) "{escaped}")
  (func (export "_start")
    (i32.store (i32.const 0) (i32.const 16))
    (i32.store (i32.const 4) (i32.const {len}))
    (drop (call $fd_write (i32.const 1) (i32.const 0) (i32.const 1) (i32.const 8)))))"#,
        pages = (bytes.len() + 16) / 65536 + 1,
        len = bytes.len()
    )
}

/// A module answering with `document`.
fn returning(document: &[u8]) -> String {
    writing(&Response::Document(document.to_vec()).encode().unwrap())
}

/// One-section PDF where `objects[i]` is object `i + 1` (as in the core's
/// tests).
fn build(objects: &[&str]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let startxref = out.len();
    let size = offsets.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{startxref}\n%%EOF\n")
            .as_bytes(),
    );
    out
}

// ---------------------------------------------------------------------------
// Time, memory, output
// ---------------------------------------------------------------------------

#[test]
fn a_module_past_its_time_limit_is_stopped() {
    let spinning = [
        // A plain endless loop.
        r#"(module (memory (export "memory") 1)
             (func (export "_start") (loop $spin (br $spin))))"#,
        // The same loop in the start section, run while instantiating.
        r#"(module (memory (export "memory") 1)
             (func $spin (loop $again (br $again)))
             (start $spin)
             (func (export "_start")))"#,
        // An endless stream of host calls.
        r#"(module
             (import "wasi_snapshot_preview1" "sched_yield" (func $yield (result i32)))
             (memory (export "memory") 1)
             (func (export "_start") (loop $spin (drop (call $yield)) (br $spin))))"#,
    ];
    for wat in spinning {
        let module = load(manifest(DOCUMENTS, &limits(300, 16, 4), ""), wat).unwrap();
        let started = Instant::now();
        let err = run(&module, &[]).unwrap_err();
        assert!(
            matches!(err, HostError::Timeout { limit_ms: 300, .. }),
            "{err}\n{wat}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "stopped after {:?}",
            started.elapsed()
        );
    }
}

#[test]
fn a_module_past_its_memory_limit_is_stopped() {
    let growing = [
        // Grows page by page until refused.
        r#"(module (memory (export "memory") 1)
             (func (export "_start")
               (loop $grow (br_if $grow (i32.ne (memory.grow (i32.const 1)) (i32.const -1))))))"#,
        // Asks for more than the limit from the start.
        r#"(module (memory (export "memory") 100) (func (export "_start")))"#,
    ];
    for wat in growing {
        let module = load(manifest(DOCUMENTS, &limits(10_000, 2, 4), ""), wat).unwrap();
        let err = run(&module, &[]).unwrap_err();
        assert!(
            matches!(err, HostError::MemoryExceeded { limit_mib: 2, .. }),
            "{err}\n{wat}"
        );
    }
}

#[test]
fn a_module_writing_past_its_output_limit_is_stopped() {
    let wat = format!(
        r#"(module
  {FD_WRITE}
  (memory (export "memory") 2)
  (func (export "_start")
    (i32.store (i32.const 0) (i32.const 16))
    (i32.store (i32.const 4) (i32.const 65536))
    (loop $write
      (drop (call $fd_write (i32.const 1) (i32.const 0) (i32.const 1) (i32.const 8)))
      (br $write))))"#
    );
    let module = load(manifest(DOCUMENTS, &limits(10_000, 16, 1), ""), &wat).unwrap();
    let err = run(&module, &[]).unwrap_err();
    assert!(
        matches!(err, HostError::OutputTooLarge { limit_mib: 1, .. }),
        "{err}"
    );
}

// ---------------------------------------------------------------------------
// What all the runs of a host share
// ---------------------------------------------------------------------------

fn host_with(change: impl FnOnce(&mut HostLimits)) -> Host {
    let mut limits = HostLimits::default();
    change(&mut limits);
    Host::with_limits(limits).unwrap()
}

const IDLE: &str = r#"(module (memory (export "memory") 1) (func (export "_start")))"#;

#[test]
fn limits_above_the_host_ceilings_are_refused_at_load() {
    let idle = wat::parse_str(IDLE).unwrap();
    // The largest time limit TOML can spell: 292 million years.
    let endless = manifest(
        DOCUMENTS,
        &limits(u64::try_from(i64::MAX).unwrap(), 16, 4),
        "",
    );
    assert!(matches!(
        Host::new().unwrap().load_bytes(endless, &idle),
        Err(HostError::LimitAboveCeiling {
            limit: "timeout_ms",
            ..
        })
    ));
    let host = host_with(|l| {
        l.memory_budget_mib = 64;
        l.max_output_mib = 8;
    });
    // More memory than the whole budget could never be given.
    assert!(matches!(
        host.load_bytes(manifest(DOCUMENTS, &limits(1000, 65, 4), ""), &idle),
        Err(HostError::LimitAboveCeiling {
            limit: "memory_mib",
            ceiling: 64,
            ..
        })
    ));
    assert!(matches!(
        host.load_bytes(manifest(DOCUMENTS, &limits(1000, 16, 9), ""), &idle),
        Err(HostError::LimitAboveCeiling {
            limit: "max_output_mib",
            ..
        })
    ));
    assert!(host
        .load_bytes(manifest(DOCUMENTS, &limits(1000, 64, 8), ""), &idle)
        .is_ok());
    // The merge module's own limits fit the default ceilings.
    let merge =
        Manifest::from_toml(&std::fs::read_to_string(root_path_of_merge_manifest()).unwrap())
            .unwrap();
    assert!(Host::new().unwrap().load_bytes(merge, &idle).is_ok());
}

#[test]
fn the_memory_budget_is_shared_by_the_runs_of_a_host() {
    let host = host_with(|l| {
        l.memory_budget_mib = 64;
        l.max_concurrent_runs = 4;
    });
    let run_limits = limits(2000, 48, 4);
    // Grows to 641 pages (40 MiB), then spins until its time limit.
    let holding = host
        .load_bytes(
            manifest(DOCUMENTS, &run_limits, ""),
            &wat::parse_str(
                r#"(module (memory (export "memory") 1)
                     (func (export "_start") (drop (memory.grow (i32.const 640))) (loop $spin (br $spin))))"#,
            )
            .unwrap(),
        )
        .unwrap();
    // Grows the same, then exits with 7. Loaded through a clone: clones
    // share the budget.
    let growing = host
        .clone()
        .load_bytes(
            manifest(DOCUMENTS, &run_limits, ""),
            &wat::parse_str(format!(
                r#"(module {PROC_EXIT} (memory (export "memory") 1)
                     (func (export "_start") (drop (memory.grow (i32.const 640))) (call $proc_exit (i32.const 7))))"#
            ))
            .unwrap(),
        )
        .unwrap();
    // Alone, each module gets its 40 MiB: 48 is within its limit.
    assert!(matches!(
        run(&growing, &[]),
        Err(HostError::Exited { code: 7, .. })
    ));
    // Nothing tells the test when the first module has grown: the second
    // tries until refused. On a loaded machine it may run before, and
    // rarely at the very moment the first grows, which is then the one
    // refused: the scenario starts over.
    let refused = (0..3).any(|_| {
        std::thread::scope(|s| {
            let first = s.spawn(|| run(&holding, &[]));
            let started = Instant::now();
            let mut refused = false;
            while started.elapsed() < Duration::from_millis(1500) {
                match run(&growing, &[]) {
                    Err(HostError::HostMemoryExhausted { budget_mib: 64, .. }) => {
                        refused = true;
                        break;
                    }
                    Err(HostError::Exited { code: 7, .. }) => {
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    other => panic!("{other:?}"),
                }
            }
            match first.join().unwrap() {
                Err(HostError::Timeout { .. }) => refused,
                Err(HostError::HostMemoryExhausted { .. }) => false,
                other => panic!("{other:?}"),
            }
        })
    });
    assert!(refused, "the second module was never refused");
    // The first run gave its share back when it ended.
    assert!(matches!(
        run(&growing, &[]),
        Err(HostError::Exited { code: 7, .. })
    ));
}

#[test]
fn the_answer_counts_in_the_memory_budget() {
    let host = host_with(|l| {
        l.memory_budget_mib = 8;
        l.max_concurrent_runs = 1;
    });
    // 5 MiB of memory, written out almost whole: about 10 MiB for one run,
    // each part within the module's own limits.
    let wat = format!(
        r#"(module {FD_WRITE} (memory (export "memory") 80)
  (func (export "_start")
    (i32.store (i32.const 0) (i32.const 16))
    (i32.store (i32.const 4) (i32.const 5000000))
    (drop (call $fd_write (i32.const 1) (i32.const 0) (i32.const 1) (i32.const 8)))))"#
    );
    let module = host
        .load_bytes(
            manifest(DOCUMENTS, &limits(10_000, 8, 8), ""),
            &wat::parse_str(wat).unwrap(),
        )
        .unwrap();
    let err = run(&module, &[]).unwrap_err();
    assert!(
        matches!(err, HostError::HostMemoryExhausted { budget_mib: 8, .. }),
        "{err}"
    );
}

#[test]
fn revalidation_takes_its_share_of_the_budget_before_it_starts() {
    // A valid one-page document of about 1 MiB: the answer (1 MiB) and
    // its re-validation (6 MiB) do not fit a budget of 4 MiB.
    let pdf = build(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] >>",
        &format!("({})", "A".repeat(1 << 20)),
    ]);
    let wat = returning(&pdf);
    let tight = host_with(|l| {
        l.memory_budget_mib = 4;
        l.max_concurrent_runs = 1;
    });
    let module = tight
        .load_bytes(
            manifest(DOCUMENTS, &limits(10_000, 4, 4), ""),
            &wat::parse_str(&wat).unwrap(),
        )
        .unwrap();
    let err = run(&module, &[]).unwrap_err();
    assert!(
        matches!(err, HostError::HostMemoryExhausted { budget_mib: 4, .. }),
        "{err}"
    );
    // With room for it, the same answer is accepted.
    let roomy = host_with(|l| l.memory_budget_mib = 16);
    let module = roomy
        .load_bytes(
            manifest(DOCUMENTS, &limits(10_000, 4, 4), ""),
            &wat::parse_str(&wat).unwrap(),
        )
        .unwrap();
    assert!(run(&module, &[]).is_ok());
}

#[test]
fn runs_past_the_concurrency_limit_wait_their_turn() {
    let host = host_with(|l| l.max_concurrent_runs = 1);
    let module = host
        .load_bytes(
            manifest(DOCUMENTS, &limits(400, 16, 4), ""),
            &wat::parse_str(
                r#"(module (memory (export "memory") 1) (func (export "_start") (loop $spin (br $spin))))"#,
            )
            .unwrap(),
        )
        .unwrap();
    let started = Instant::now();
    let results: Vec<(Result<RunOutput, HostError>, Duration)> = std::thread::scope(|s| {
        let runs: Vec<_> = (0..2)
            .map(|_| s.spawn(|| (run(&module, &[]), started.elapsed())))
            .collect();
        runs.into_iter().map(|r| r.join().unwrap()).collect()
    });
    // Each had its whole time limit, one after the other: the time limit
    // starts when a run starts, not when it is asked for.
    for (result, _) in &results {
        assert!(
            matches!(result, Err(HostError::Timeout { limit_ms: 400, .. })),
            "{result:?}"
        );
    }
    let last = results.iter().map(|(_, t)| *t).max().unwrap();
    assert!(last >= Duration::from_millis(790), "{last:?}");
}

// ---------------------------------------------------------------------------
// Capabilities
// ---------------------------------------------------------------------------

#[test]
fn a_module_calling_an_undeclared_capability_is_stopped() {
    let attempts = [
        (
            "wasi_snapshot_preview1",
            "path_open",
            "(param i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i32)",
            "(drop (call $f (i32.const 3) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0) (i64.const 0) (i64.const 0) (i32.const 0) (i32.const 0)))",
        ),
        (
            "wasi_snapshot_preview1",
            "sock_accept",
            "(param i32 i32 i32) (result i32)",
            "(drop (call $f (i32.const 3) (i32.const 0) (i32.const 0)))",
        ),
        (
            "wasi_snapshot_preview1",
            "clock_time_get",
            "(param i32 i64 i32) (result i32)",
            "(drop (call $f (i32.const 0) (i64.const 1) (i32.const 0)))",
        ),
        (
            "wasi_snapshot_preview1",
            "poll_oneoff",
            "(param i32 i32 i32 i32) (result i32)",
            "(drop (call $f (i32.const 0) (i32.const 64) (i32.const 1) (i32.const 128)))",
        ),
        ("env", "system", "(param i32) (result i32)", "(drop (call $f (i32.const 0)))"),
    ];
    for (namespace, function, signature, call) in attempts {
        let wat = format!(
            r#"(module
  (import "{namespace}" "{function}" (func $f {signature}))
  (memory (export "memory") 1)
  (func (export "_start") {call}))"#
        );
        // Loading succeeds: an import alone is harmless, calling it is not.
        let module = load(default_manifest(), &wat).unwrap();
        let err = run(&module, &[]).unwrap_err();
        let expected = format!("{namespace}::{function}");
        assert!(
            matches!(&err, HostError::CapabilityDenied { import, .. } if *import == expected),
            "{expected}: {err}"
        );
    }
}

#[test]
fn provided_wasi_functions_give_nothing_away() {
    // No preopened directory: fd_prestat_get on the first candidate (3)
    // answers EBADF (8), which the module turns into its exit code.
    let prestat = format!(
        r#"(module
  (import "wasi_snapshot_preview1" "fd_prestat_get" (func $prestat (param i32 i32) (result i32)))
  {PROC_EXIT}
  (memory (export "memory") 1)
  (func (export "_start") (call $proc_exit (call $prestat (i32.const 3) (i32.const 0)))))"#
    );
    let module = load(default_manifest(), &prestat).unwrap();
    assert!(matches!(
        run(&module, &[]),
        Err(HostError::Exited { code: 8, .. })
    ));

    // Empty environment and arguments: the sizes written over 7 and 7 are
    // zero, the exit code is 100 + their sum.
    let environment = format!(
        r#"(module
  (import "wasi_snapshot_preview1" "environ_sizes_get" (func $env (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "args_sizes_get" (func $args (param i32 i32) (result i32)))
  {PROC_EXIT}
  (memory (export "memory") 1)
  (data (i32.const 0) "\07\00\00\00\07\00\00\00\07\00\00\00\07\00\00\00")
  (func (export "_start")
    (drop (call $env (i32.const 0) (i32.const 4)))
    (drop (call $args (i32.const 8) (i32.const 12)))
    (call $proc_exit
      (i32.add (i32.const 100)
        (i32.add (i32.add (i32.load (i32.const 0)) (i32.load (i32.const 4)))
                 (i32.add (i32.load (i32.const 8)) (i32.load (i32.const 12))))))))"#
    );
    let module = load(default_manifest(), &environment).unwrap();
    let err = run(&module, &[]).unwrap_err();
    assert!(matches!(err, HostError::Exited { code: 100, .. }), "{err}");

    // Writing to a descriptor other than 1 and 2 is EBADF.
    let other_fd = format!(
        r#"(module
  {FD_WRITE}
  {PROC_EXIT}
  (memory (export "memory") 1)
  (func (export "_start") (call $proc_exit (call $fd_write (i32.const 3) (i32.const 0) (i32.const 0) (i32.const 8)))))"#
    );
    let module = load(default_manifest(), &other_fd).unwrap();
    assert!(matches!(
        run(&module, &[]),
        Err(HostError::Exited { code: 8, .. })
    ));
}

#[test]
fn bad_pointers_are_errors_for_the_module_not_crashes_of_the_host() {
    // An iovec array past the end of memory: EFAULT (21).
    let iovecs = format!(
        r#"(module
  {FD_WRITE}
  {PROC_EXIT}
  (memory (export "memory") 1)
  (func (export "_start")
    (call $proc_exit (call $fd_write (i32.const 1) (i32.const -16) (i32.const 1) (i32.const 8)))))"#
    );
    // A read buffer of 100 bytes starting 6 bytes before the end of memory.
    let buffer = r#"(module
  (import "wasi_snapshot_preview1" "fd_read" (func $fd_read (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
  (memory (export "memory") 1)
  (func (export "_start")
    (i32.store (i32.const 0) (i32.const 65530))
    (i32.store (i32.const 4) (i32.const 100))
    (call $proc_exit (call $fd_read (i32.const 0) (i32.const 0) (i32.const 1) (i32.const 8)))))"#;
    // random_get over a range that wraps around the address space.
    let random = r#"(module
  (import "wasi_snapshot_preview1" "random_get" (func $random (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
  (memory (export "memory") 1)
  (func (export "_start") (call $proc_exit (call $random (i32.const -1) (i32.const -1)))))"#;
    for wat in [iovecs.as_str(), buffer, random] {
        let module = load(default_manifest(), wat).unwrap();
        let err = run(&module, &[b"%PDF"]).unwrap_err();
        assert!(
            matches!(err, HostError::Exited { code: 21, .. }),
            "{err}\n{wat}"
        );
    }
}

/// Characters that act on a terminal or on the direction of text instead
/// of being shown.
fn hidden(c: char) -> bool {
    c.is_control() && c != '\n' && c != '\t'
        || matches!(c, '\u{200e}' | '\u{200f}' | '\u{061c}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

#[test]
fn text_from_a_module_is_bounded_and_inert() {
    // Clears the screen, retitles the terminal, reverses what follows.
    let escapes = "\u{1b}[2J\u{1b}]0;owned\u{7}\r\u{202e}fdp.exe";
    let long = format!("{escapes}{}", "A".repeat(1 << 20));

    // An error answer: the message a UI shows.
    let module = load(
        default_manifest(),
        &writing(&Response::Error(long.clone()).encode().unwrap()),
    )
    .unwrap();
    let err = run(&module, &[]).unwrap_err();
    let HostError::ModuleFailed { message, .. } = &err else {
        panic!("{err}")
    };
    assert!(message.len() <= 8 << 10, "{} bytes kept", message.len());
    assert!(!message.chars().any(hidden), "{message:?}");
    assert!(!err.to_string().chars().any(hidden));

    // Standard error, kept as diagnostics.
    let wat = format!(
        r#"(module
  {FD_WRITE}
  (memory (export "memory") 1)
  (data (i32.const 16) "{}")
  (func (export "_start")
    (i32.store (i32.const 0) (i32.const 16))
    (i32.store (i32.const 4) (i32.const {}))
    (drop (call $fd_write (i32.const 2) (i32.const 0) (i32.const 1) (i32.const 8)))
    unreachable))"#,
        escapes
            .bytes()
            .map(|b| format!("\\{b:02x}"))
            .collect::<String>(),
        escapes.len()
    );
    let err = run(&load(default_manifest(), &wat).unwrap(), &[]).unwrap_err();
    let HostError::Trapped { diagnostics, .. } = &err else {
        panic!("{err}")
    };
    assert!(diagnostics.contains("fdp.exe"), "{diagnostics:?}");
    assert!(!diagnostics.chars().any(hidden), "{diagnostics:?}");
    assert!(!err.to_string().chars().any(hidden));

    // The name of an import the host denies.
    let wat = r#"(module (import "env" "\1b[2Jsystem" (func $f))
      (memory (export "memory") 1) (func (export "_start") (call $f)))"#;
    let err = run(&load(default_manifest(), wat).unwrap(), &[]).unwrap_err();
    let HostError::CapabilityDenied { import, .. } = &err else {
        panic!("{err}")
    };
    assert!(!import.chars().any(hidden), "{import:?}");

    // A manifest whose identity would carry the same: refused at load.
    let idle = r#"(module (memory (export "memory") 1) (func (export "_start")))"#;
    for field in ["id", "name", "version", "action", "label"] {
        let mut m = default_manifest();
        let evil = format!("org.example{escapes}");
        match field {
            "id" => m.id = evil,
            "name" => m.name = evil,
            "version" => m.version = evil,
            "action" => m.actions[0].id = evil,
            _ => m.actions[0].label = evil,
        }
        assert!(
            matches!(load(m, idle), Err(HostError::Manifest { .. })),
            "{field}"
        );
    }
}

#[test]
fn tables_past_the_host_ceiling_are_refused() {
    for wat in [
        // One table of two million elements (the host allows 1 << 20).
        r#"(module (table 2000000 funcref) (memory (export "memory") 1) (func (export "_start")))"#,
        // Nine tables (the host allows 8).
        r#"(module (table 1 funcref) (table 1 funcref) (table 1 funcref) (table 1 funcref)
             (table 1 funcref) (table 1 funcref) (table 1 funcref) (table 1 funcref) (table 1 funcref)
             (memory (export "memory") 1) (func (export "_start")))"#,
        // Two memories: the limiter sees one only.
        r#"(module (memory (export "memory") 1) (memory 1) (func (export "_start")))"#,
    ] {
        let module = load(default_manifest(), wat).unwrap();
        let err = run(&module, &[]).unwrap_err();
        assert!(
            matches!(
                err,
                HostError::MemoryExceeded { .. } | HostError::Trapped { .. }
            ),
            "{err}\n{wat}"
        );
    }
}

#[test]
fn permissions_are_checked_by_the_host() {
    let idle = r#"(module (memory (export "memory") 1) (func (export "_start")))"#;
    // A permission this host cannot grant: refused at load.
    let network = manifest(
        r#"{ kind = "read_document" }, { kind = "network", hosts = ["api.example.org"] }"#,
        &limits(1000, 16, 4),
        "",
    );
    assert!(matches!(
        load(network, idle),
        Err(HostError::PermissionUnavailable { .. })
    ));
    // Documents to a module that does not declare read_document.
    let module = load(
        manifest(r#"{ kind = "write_document" }"#, &limits(1000, 16, 4), ""),
        idle,
    )
    .unwrap();
    let err = run(&module, &[b"%PDF-1.7"]).unwrap_err();
    assert!(
        matches!(&err, HostError::PermissionNotDeclared { permission, .. } if permission == "read_document"),
        "{err}"
    );
    // A document from a module that does not declare write_document.
    let pdf = build(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] >>",
    ]);
    let module = load(
        manifest(r#"{ kind = "read_document" }"#, &limits(1000, 16, 4), ""),
        &returning(&pdf),
    )
    .unwrap();
    let err = run(&module, &[]).unwrap_err();
    assert!(
        matches!(&err, HostError::PermissionNotDeclared { permission, .. } if permission == "write_document"),
        "{err}"
    );
}

#[test]
fn parameters_are_checked_before_the_module_starts() {
    let params = r#"
[[actions.params]]
id = "every"
kind = "integer"
required = true
min = 1
max = 100

[[actions.params]]
id = "title"
kind = "text"
"#;
    // The module traps as soon as it runs: any error but a trap proves the
    // host stopped the run before.
    let module = load(
        manifest(DOCUMENTS, &limits(1000, 16, 4), params),
        r#"(module (memory (export "memory") 1) (func (export "_start") unreachable))"#,
    )
    .unwrap();
    let given = |pairs: &[(&str, ParamValue)]| -> BTreeMap<String, ParamValue> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    };
    for (params, why) in [
        (given(&[]), "missing"),
        (given(&[("every", ParamValue::Integer(0))]), "below min"),
        (given(&[("every", ParamValue::Integer(101))]), "above max"),
        (
            given(&[("every", ParamValue::Text("3".into()))]),
            "wrong kind",
        ),
        (
            given(&[
                ("every", ParamValue::Integer(3)),
                ("other", ParamValue::Boolean(true)),
            ]),
            "unknown",
        ),
    ] {
        let err = module.run("run", &params, &[]).unwrap_err();
        assert!(
            matches!(err, HostError::BadParameter { .. }),
            "{why}: {err}"
        );
    }
    let accepted = given(&[
        ("every", ParamValue::Integer(3)),
        ("title", ParamValue::Text("x".into())),
    ]);
    let err = module.run("run", &accepted, &[]).unwrap_err();
    assert!(matches!(err, HostError::Trapped { .. }), "{err}");
    assert!(matches!(
        module.run("nothing", &BTreeMap::new(), &[]),
        Err(HostError::UnknownAction { .. })
    ));
}

#[test]
fn malformed_modules_are_refused_at_load() {
    let refused = [
        ("no _start", r#"(module (memory (export "memory") 1))"#.to_string()),
        ("no memory", r#"(module (func (export "_start")))"#.to_string()),
        (
            "shared memory",
            r#"(module (memory (export "memory") 1 1 shared) (func (export "_start")))"#
                .to_string(),
        ),
        (
            "imported memory",
            r#"(module (import "env" "memory" (memory 1)) (export "memory" (memory 0)) (func (export "_start")))"#
                .to_string(),
        ),
        (
            "WASI function with a wrong signature",
            r#"(module (import "wasi_snapshot_preview1" "fd_write" (func (param i32)))
                 (memory (export "memory") 1) (func (export "_start")))"#
                .to_string(),
        ),
    ];
    for (why, wat) in refused {
        let wasm = match wat::parse_str(&wat) {
            Ok(wasm) => wasm,
            // The text parser may refuse what the runtime would: fine.
            Err(_) => continue,
        };
        let err = Host::new()
            .unwrap()
            .load_bytes(default_manifest(), &wasm)
            .unwrap_err();
        assert!(matches!(err, HostError::BadModule { .. }), "{why}: {err}");
    }
    let err = Host::new()
        .unwrap()
        .load_bytes(default_manifest(), b"%PDF-1.7 is not WebAssembly")
        .unwrap_err();
    assert!(matches!(err, HostError::BadModule { .. }), "{err}");
}

// ---------------------------------------------------------------------------
// Re-validation
// ---------------------------------------------------------------------------

#[test]
fn a_corrupt_document_is_rejected_at_revalidation() {
    for corrupt in [
        b"this is not a PDF".to_vec(),
        // A catalog without a page tree.
        b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\nendobj\ntrailer\n<< /Root 1 0 R >>\n%%EOF\n"
            .to_vec(),
        // A page tree without a page.
        build(&[
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [] /Count 0 >>",
        ]),
    ] {
        let module = load(default_manifest(), &returning(&corrupt)).unwrap();
        let err = run(&module, &[]).unwrap_err();
        assert!(matches!(err, HostError::Rejected { .. }), "{err}");
    }
}

#[test]
fn revalidating_the_deepest_document_fits_the_smallest_caller_stack() {
    // `run` re-validates on the caller's thread. A Windows main thread has
    // 1 MiB of stack; a stack overflow aborts the process. The core
    // parses and writes nesting up to parser::MAX_DEPTH (256).
    let depth = fyp_core::parser::MAX_DEPTH;
    let nested = format!("{}{}", "[".repeat(depth - 2), "]".repeat(depth - 2));
    let pdf = build(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        &format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Deep {nested} >>"),
    ]);
    let out = std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(move || fyp_host::revalidate(&pdf).map(|(doc, _)| doc.len()))
        .unwrap()
        .join()
        .unwrap();
    assert!(out.is_ok(), "{out:?}");
}

#[test]
fn an_unreadable_object_never_reaches_the_caller() {
    // Object 4 is listed in a sound table but does not parse; the page
    // refers to it.
    let injected = build(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Annots [4 0 R] >>",
        "<< /Subtype /Link /Oops (never closed",
    ]);
    let module = load(default_manifest(), &returning(&injected)).unwrap();
    let out = run(&module, &[]).unwrap();
    assert_ne!(out.document, injected);
    let doc = Document::open(&out.document).unwrap();
    assert_eq!(doc.reconstructed(), None);
    assert_eq!(doc.page_count(), Ok(1));
    assert_eq!(doc.get(ObjRef { num: 4, gen: 0 }), Ok(None));

    // A document without `startxref` is reconstructed by scanning, then
    // rewritten: accepted, and the repair is reported. (A wrong
    // `startxref` would not do: the core finds a table next to it.)
    let mut broken = injected.clone();
    let at = broken.windows(9).rposition(|w| w == b"startxref").unwrap();
    broken.truncate(at);
    broken.extend_from_slice(b"%%EOF\n");
    let module = load(default_manifest(), &returning(&broken)).unwrap();
    let out = run(&module, &[]).unwrap();
    assert!(out.reconstructed.is_some());
    let doc = Document::open(&out.document).unwrap();
    assert_eq!(doc.reconstructed(), None);
    assert_eq!(doc.page_count(), Ok(1));
}

#[test]
fn module_errors_and_missing_answers_are_reported() {
    let failing = writing(
        &Response::Error("pas de page à fusionner".into())
            .encode()
            .unwrap(),
    );
    let module = load(default_manifest(), &failing).unwrap();
    assert!(matches!(
        run(&module, &[]),
        Err(HostError::ModuleFailed { message, .. }) if message == "pas de page à fusionner"
    ));
    let silent = r#"(module (memory (export "memory") 1) (func (export "_start")))"#;
    let module = load(default_manifest(), silent).unwrap();
    assert!(matches!(
        run(&module, &[]),
        Err(HostError::BadResponse { .. })
    ));
    let garbage = writing(b"hello");
    let module = load(default_manifest(), &garbage).unwrap();
    assert!(matches!(
        run(&module, &[]),
        Err(HostError::BadResponse { .. })
    ));
    // A panic in Rust is an `unreachable` trap after a message on stderr.
    let panicking = r#"(module
  (import "wasi_snapshot_preview1" "fd_write" (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 16) "panicked at src/main.rs")
  (func (export "_start")
    (i32.store (i32.const 0) (i32.const 16))
    (i32.store (i32.const 4) (i32.const 23))
    (drop (call $fd_write (i32.const 2) (i32.const 0) (i32.const 1) (i32.const 8)))
    unreachable))"#;
    let module = load(default_manifest(), panicking).unwrap();
    let err = run(&module, &[]).unwrap_err();
    assert!(
        matches!(&err, HostError::Trapped { diagnostics, .. } if diagnostics == "panicked at src/main.rs"),
        "{err}"
    );
}

// ---------------------------------------------------------------------------
// Discovery and provenance
// ---------------------------------------------------------------------------

/// An empty directory of its own under the system's temporary directory.
fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fyp-host-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `root/<name>/` holding `manifest` and a module answering `message` as
/// its error.
fn install(root: &Path, name: &str, manifest: &str, message: &str) {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("manifest.toml"), manifest).unwrap();
    let wat = writing(&Response::Error(message.into()).encode().unwrap());
    std::fs::write(
        dir.join(fyp_host::MODULE_FILE),
        wat::parse_str(wat).unwrap(),
    )
    .unwrap();
}

#[test]
fn known_gap_an_impostor_of_a_repository_module_is_not_told_apart() {
    // ADR 0003, « Limites connues »: modules are not signed yet. A module
    // reusing the merge module's identifier and manifest is discovered,
    // loaded and run like the real one; only its code differs. When
    // signatures exist, this test must fail and be turned around.
    let root = scratch_dir("impostor");
    let merge_manifest = std::fs::read_to_string(root_path_of_merge_manifest()).unwrap();
    install(&root, "impostor", &merge_manifest, "impostor answering");
    let one = fixture("minimal.pdf");
    for trusted in [false, true] {
        let (found, errors) = discover(&root, trusted);
        assert!(errors.is_empty(), "{errors:?}");
        let [impostor] = found.as_slice() else {
            panic!("{found:?}")
        };
        assert_eq!(impostor.manifest.id, "org.4youpdf.merge");
        // `trusted` is the caller's word for the directory, not a check.
        assert_eq!(impostor.trusted, trusted);
        let module = Host::new().unwrap().load(impostor).unwrap();
        let err = module
            .run("merge", &BTreeMap::new(), &[&one, &one])
            .unwrap_err();
        assert!(
            matches!(&err, HostError::ModuleFailed { message, .. } if message == "impostor answering"),
            "{err}"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

fn root_path_of_merge_manifest() -> PathBuf {
    root().join("plugins/merge/manifest.toml")
}

#[test]
fn modules_sharing_an_identifier_are_all_refused() {
    let root = scratch_dir("duplicate");
    let text = std::fs::read_to_string(root_path_of_merge_manifest()).unwrap();
    // "a-" sorts before "merge": first in a directory listing on most
    // filesystems, the copy a careless lookup would pick.
    install(&root, "a-shadow", &text, "shadow");
    install(&root, "merge", &text, "real");
    let other = text.replace("org.4youpdf.merge", "org.example.other");
    install(&root, "other", &other, "other");
    let (found, errors) = discover(&root, false);
    let ids: Vec<&str> = found.iter().map(|m| m.manifest.id.as_str()).collect();
    assert_eq!(ids, ["org.example.other"]);
    assert!(
        matches!(errors.as_slice(), [HostError::DuplicateId { id, dirs }]
            if id == "org.4youpdf.merge" && dirs.len() == 2),
        "{errors:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_oversized_manifest_is_refused_without_reading_it_whole() {
    let root = scratch_dir("oversized");
    let text = std::fs::read_to_string(root_path_of_merge_manifest()).unwrap();
    // A valid manifest followed by a comment past the limit.
    let padded = format!(
        "{text}\n#{}\n",
        "x".repeat(usize::try_from(fyp_host::MAX_MANIFEST_BYTES).unwrap())
    );
    install(&root, "padded", &padded, "never runs");
    let (found, errors) = discover(&root, false);
    assert!(found.is_empty());
    assert!(
        matches!(errors.as_slice(), [HostError::Manifest { .. }]),
        "{errors:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------
// The real merge module
// ---------------------------------------------------------------------------

fn merge_module() -> Option<LoadedModule> {
    let plugins = root().join("plugins");
    if !plugins.join("merge").join(fyp_host::MODULE_FILE).is_file() {
        assert!(
            std::env::var_os("FYP_REQUIRE_MODULES").is_none(),
            "plugins/merge/module.wasm missing: run python tools/build_modules.py"
        );
        eprintln!("plugins/merge/module.wasm not built (python tools/build_modules.py): skipped");
        return None;
    }
    let (found, errors) = discover(&plugins, false);
    assert!(errors.is_empty(), "{errors:?}");
    let merge = found
        .iter()
        .find(|m| m.manifest.id == "org.4youpdf.merge")
        .expect("merge module discovered");
    Some(Host::new().unwrap().load(merge).unwrap())
}

#[test]
fn the_merge_module_gives_the_same_bytes_as_a_direct_merge() {
    let Some(module) = merge_module() else {
        return;
    };
    let mut sets: Vec<Vec<(String, Vec<u8>)>> = [
        &["minimal.pdf", "xrefstream.pdf", "objstm.pdf"][..],
        &["objstm.pdf", "minimal.pdf"],
        &["incremental.pdf", "hybrid.pdf", "encrypted-aes256.pdf"],
    ]
    .iter()
    .map(|names| names.iter().map(|n| (n.to_string(), fixture(n))).collect())
    .collect();
    let corpus: Vec<(String, Vec<u8>)> =
        ["qpdf/outlines-with-actions.pdf", "pdfjs/tracemonkey.pdf"]
            .iter()
            .filter_map(|rel| {
                let bytes = std::fs::read(root().join("tests/corpus").join(rel)).ok()?;
                Some((rel.to_string(), bytes))
            })
            .collect();
    if corpus.len() == 2 {
        sets.push(corpus);
    }
    for set in sets {
        let names: Vec<&str> = set.iter().map(|(n, _)| n.as_str()).collect();
        let docs: Vec<Document<'_>> = set
            .iter()
            .map(|(_, bytes)| Document::open(bytes).unwrap())
            .collect();
        let direct = ops::merge(&docs).unwrap();
        let inputs: Vec<&[u8]> = set.iter().map(|(_, bytes)| bytes.as_slice()).collect();
        let started = Instant::now();
        let out = module
            .run("merge", &BTreeMap::new(), &inputs)
            .unwrap_or_else(|e| panic!("{names:?}: {e}"));
        eprintln!(
            "{names:?}: {} bytes in {:?}",
            out.document.len(),
            started.elapsed()
        );
        assert_eq!(out.reconstructed, None, "{names:?}");
        assert!(
            out.document == direct,
            "{names:?}: bytes differ from ops::merge"
        );
    }
}

#[test]
fn the_merge_module_refuses_what_its_manifest_does_not_allow() {
    let Some(module) = merge_module() else {
        return;
    };
    let one = fixture("minimal.pdf");
    assert!(matches!(
        module.run("merge", &BTreeMap::new(), &[&one]),
        Err(HostError::NotEnoughInputs {
            min: 2,
            given: 1,
            ..
        })
    ));
    let mut params = BTreeMap::new();
    params.insert("pages".to_string(), ParamValue::Integer(1));
    assert!(matches!(
        module.run("merge", &params, &[&one, &one]),
        Err(HostError::BadParameter { .. })
    ));
    // A document the module's core cannot open: its error, reported.
    let err = module
        .run("merge", &BTreeMap::new(), &[&one, b"not a pdf"])
        .unwrap_err();
    assert!(
        matches!(&err, HostError::ModuleFailed { message, .. } if message.contains("document 2")),
        "{err}"
    );
}
