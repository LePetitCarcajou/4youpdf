//! Measures behind ADR 0003, « Limites connues »: runs of modules against
//! the host's limits, re-validation of large answers, compilation of
//! hostile modules.
//!
//! ```text
//! cargo run --release --manifest-path tools/bench_host/Cargo.toml -- <command>
//! ```
//!
//! Every measure prints wall time, CPU time and the peaks of working set and
//! committed memory, sampled every 2 ms: lower bounds. Run one measure per
//! process, since the peaks are the process's. Some compile variants take
//! minutes and several GiB (`branches` did not finish in 240 s): run them
//! under a watchdog. The host is built with the default `HostLimits`.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::Path;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

use fyp_host::{discover, Host, HostError};
use fyp_plugin_api::exchange::Response;
use fyp_plugin_api::Manifest;

const USAGE: &str = "\
usage: bench_host <command>
  stack <n> <a.pdf> <b.pdf>          n concurrent runs of plugins/merge on a and b (needs module.wasm)
  hog <n>                            n concurrent modules at the merge limits: 512 MiB touched, spin to a 3 s timeout
  output <mib>                       one module answering a <mib> MiB document of empty objects
  revalidate <mib> objects|garbage   fyp_host::revalidate alone on <mib> MiB
  compile <variant>                  Host::load_bytes on a hostile module:
                                       arith | branches | nest | brtable   functions of the validator's maximum size, 64 MiB in all
                                       one-<kind>[:<KiB>]                  one function of that kind (and size)
                                       many-funcs[:<n>]                    n functions of dead code (1,000,000 by default)
  inspect <module.wasm>              functions, largest body, nesting, locals: to compare a real module with a hostile one";

const KINDS: [&str; 4] = ["arith", "branches", "nest", "brtable"];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    let ok = match words.as_slice() {
        ["stack", n, a, b] => n.parse().map(|n| stack(n, a, b)).is_ok(),
        ["hog", n] => n.parse().map(hog).is_ok(),
        ["output", mib] => mib.parse().map(output).is_ok(),
        ["revalidate", mib, pattern @ ("objects" | "garbage")] => {
            mib.parse().map(|mib| revalidate(mib, pattern)).is_ok()
        }
        ["compile", variant] => compile(variant),
        ["inspect", path] => match std::fs::read(path) {
            Ok(wasm) => {
                inspect(&wasm);
                true
            }
            Err(e) => {
                eprintln!("{path}: {e}");
                return ExitCode::FAILURE;
            }
        },
        _ => false,
    };
    if ok {
        ExitCode::SUCCESS
    } else {
        eprintln!("{USAGE}");
        ExitCode::FAILURE
    }
}

// --- measuring -----------------------------------------------------------

/// Working set and committed memory of this process, in bytes.
fn memory() -> (usize, usize) {
    memory_stats::memory_stats()
        .map(|m| (m.physical_mem, m.virtual_mem))
        .unwrap_or((0, 0))
}

/// Wall time, CPU time and memory peaks from `start` to `finish`.
struct Sampler {
    stop: Arc<AtomicBool>,
    handle: thread::JoinHandle<(usize, usize)>,
    base: (usize, usize),
    cpu: cpu_time::ProcessTime,
    wall: Instant,
}

impl Sampler {
    fn start() -> Sampler {
        let base = memory();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            let mut peak = memory();
            while !flag.load(Ordering::Relaxed) {
                let now = memory();
                peak = (peak.0.max(now.0), peak.1.max(now.1));
                thread::sleep(Duration::from_millis(2));
            }
            peak
        });
        Sampler {
            stop,
            handle,
            base,
            cpu: cpu_time::ProcessTime::now(),
            wall: Instant::now(),
        }
    }

    fn finish(self, label: &str) {
        let (wall, cpu) = (self.wall.elapsed(), self.cpu.elapsed());
        self.stop.store(true, Ordering::Relaxed);
        let peak = self.handle.join().unwrap_or(self.base);
        println!(
            "{label}: wall {:.2} s, cpu {:.2} s, peak working set {} MiB (+{}), peak commit {} MiB (+{})",
            wall.as_secs_f64(),
            cpu.as_secs_f64(),
            peak.0 >> 20,
            peak.0.saturating_sub(self.base.0) >> 20,
            peak.1 >> 20,
            peak.1.saturating_sub(self.base.1) >> 20,
        );
        std::io::stdout().flush().ok();
    }
}

/// A module's manifest: one action `run`, both document permissions.
fn manifest(limits: &str) -> Manifest {
    Manifest::from_toml(&format!(
        r#"
id = "org.example.bench"
name = "Bench"
version = "0.0.1"
api_version = "{}"
license = "AGPL-3.0-or-later"
source = ""
runtime = "wasm"
permissions = [{{ kind = "read_document" }}, {{ kind = "write_document" }}]
[limits]
{limits}
[[actions]]
id = "run"
label = "Run"
category = "process"
min_inputs = 0
"#,
        fyp_plugin_api::API_VERSION
    ))
    .expect("a valid bench manifest")
}

// --- runs ----------------------------------------------------------------

fn stack(n: usize, a: &str, b: &str) {
    let plugins = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins");
    let (found, errors) = discover(&plugins, false);
    assert!(errors.is_empty(), "{errors:?}");
    let merge = found
        .iter()
        .find(|m| m.manifest.id == "org.4youpdf.merge")
        .expect("plugins/merge discovered");
    let module = Host::new()
        .and_then(|host| host.load(merge))
        .unwrap_or_else(|e| panic!("{e}\n(python tools/build_modules.py builds module.wasm)"));
    let a = std::fs::read(a).expect("first document");
    let b = std::fs::read(b).expect("second document");
    let sampler = Sampler::start();
    let barrier = Barrier::new(n);
    let results: Vec<(Duration, Result<usize, String>)> = thread::scope(|s| {
        let runs: Vec<_> = (0..n)
            .map(|_| {
                s.spawn(|| {
                    barrier.wait();
                    let started = Instant::now();
                    let result = module
                        .run("merge", &BTreeMap::new(), &[&a, &b])
                        .map(|out| out.document.len())
                        .map_err(|e| e.to_string());
                    (started.elapsed(), result)
                })
            })
            .collect();
        runs.into_iter()
            .map(|run| run.join().expect("a run thread"))
            .collect()
    });
    let slowest = results.iter().map(|r| r.0).max().unwrap_or_default();
    let failed = results.iter().filter(|r| r.1.is_err()).count();
    sampler.finish(&format!(
        "stack n={n} inputs {}+{} KiB: slowest run {:.2} s, failed {failed}",
        a.len() >> 10,
        b.len() >> 10,
        slowest.as_secs_f64()
    ));
    if let Some((_, Err(e))) = results.iter().find(|r| r.1.is_err()) {
        println!("  first error: {e}");
    }
}

fn hog(n: usize) {
    // The merge module's memory (512 MiB), a 3 s timeout so the bench ends.
    let wat = r#"(module
      (memory (export "memory") 1)
      (func (export "_start")
        (drop (memory.grow (i32.const 8191)))
        (memory.fill (i32.const 0) (i32.const 1) (i32.const 536870912))
        (loop $spin (br $spin))))"#;
    let module = Host::new()
        .and_then(|host| {
            host.load_bytes(
                manifest("timeout_ms = 3000\nmemory_mib = 512\nmax_output_mib = 2048"),
                &wat::parse_str(wat).expect("valid WAT"),
            )
        })
        .unwrap_or_else(|e| panic!("{e}"));
    let sampler = Sampler::start();
    let barrier = Barrier::new(n);
    let outcomes: Vec<String> = thread::scope(|s| {
        let runs: Vec<_> = (0..n)
            .map(|_| {
                s.spawn(|| {
                    barrier.wait();
                    match module.run("run", &BTreeMap::new(), &[]) {
                        Err(HostError::Timeout { .. }) => "timeout".to_string(),
                        other => format!("{other:?}"),
                    }
                })
            })
            .collect();
        runs.into_iter()
            .map(|run| run.join().expect("a run thread"))
            .collect()
    });
    let timeouts = outcomes.iter().filter(|o| *o == "timeout").count();
    sampler.finish(&format!(
        "hog n={n} (512 MiB each, 3 s): {timeouts} timeouts"
    ));
    for other in outcomes.iter().filter(|o| *o != "timeout").take(3) {
        println!("  other outcome: {other}");
    }
}

/// A one-page PDF of `mib` MiB whose filler objects `N 0 obj<<>>endobj`
/// have no table: the core reconstructs, then rewrites them all.
fn filler_pdf(mib: usize) -> Vec<u8> {
    let target = mib << 20;
    let mut pdf = b"%PDF-1.7\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 10 10]>>endobj\n".to_vec();
    let mut i = 4u64;
    while pdf.len() + 64 < target {
        pdf.extend_from_slice(format!("{i} 0 obj<<>>endobj\n").as_bytes());
        i += 1;
    }
    pdf.extend_from_slice(b"trailer<</Root 1 0 R>>\n%%EOF\n");
    pdf
}

fn output(mib: usize) {
    let framed = Response::Document(filler_pdf(mib))
        .encode()
        .expect("an answer under 4 GiB");
    // The module receives its answer as the request's only document and
    // writes it back: its memory holds it once, as the merge module would.
    // The request is FYPQ, version, "run", 0 parameters, 1 document and
    // its length: 27 bytes before the document.
    let pages = framed.len() / 65536 + 2;
    let wat = format!(
        r#"(module
      (import "wasi_snapshot_preview1" "fd_write" (func $w (param i32 i32 i32 i32) (result i32)))
      (import "wasi_snapshot_preview1" "fd_read" (func $r (param i32 i32 i32 i32) (result i32)))
      (memory (export "memory") {pages})
      (func (export "_start")
        (i32.store (i32.const 0) (i32.const 16))
        (i32.store (i32.const 4) (i32.const {request}))
        (drop (call $r (i32.const 0) (i32.const 0) (i32.const 1) (i32.const 12)))
        (i32.store (i32.const 0) (i32.const 43))
        (i32.store (i32.const 4) (i32.const {len}))
        (drop (call $w (i32.const 1) (i32.const 0) (i32.const 1) (i32.const 12)))))"#,
        len = framed.len(),
        request = framed.len() + 27
    );
    let limits = format!(
        "timeout_ms = 600000\nmemory_mib = {}\nmax_output_mib = {}",
        ((pages * 65536) >> 20) + 1,
        mib + 1
    );
    let module = Host::new()
        .and_then(|host| {
            host.load_bytes(manifest(&limits), &wat::parse_str(&wat).expect("valid WAT"))
        })
        .unwrap_or_else(|e| panic!("{e}"));
    println!("answer {} MiB", framed.len() >> 20);
    let sampler = Sampler::start();
    let started = Instant::now();
    let label = match module.run("run", &BTreeMap::new(), &[&framed]) {
        Ok(out) => format!(
            "output {mib} MiB: accepted, rewrite {} MiB, in {:.2} s",
            out.document.len() >> 20,
            started.elapsed().as_secs_f64()
        ),
        Err(e) => format!(
            "output {mib} MiB: {e} after {:.2} s",
            started.elapsed().as_secs_f64()
        ),
    };
    sampler.finish(&label);
}

fn revalidate(mib: usize, pattern: &str) {
    let bytes: Vec<u8> = match pattern {
        "objects" => filler_pdf(mib),
        // Bytes without a header: the core refuses them at once.
        _ => (0..mib << 20)
            .map(|i: usize| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
            .collect(),
    };
    let sampler = Sampler::start();
    let label = match fyp_host::revalidate(&bytes) {
        Ok((doc, reconstructed)) => format!(
            "revalidate {mib} MiB {pattern}: ok, {} MiB out, reconstructed {}",
            doc.len() >> 20,
            reconstructed.is_some()
        ),
        Err(e) => format!("revalidate {mib} MiB {pattern}: {e}"),
    };
    sampler.finish(&label);
}

// --- hostile modules, hand-encoded ---------------------------------------

/// The validator's maximum function body, less room for the locals.
const MAX_BODY: usize = 7_654_321 - 16;
/// The host's maximum `module.wasm`.
const MAX_MODULE: usize = 64 << 20;

fn uleb(mut value: u64, out: &mut Vec<u8>) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn section(id: u8, payload: &[u8], out: &mut Vec<u8>) {
    out.push(id);
    uleb(payload.len() as u64, out);
    out.extend_from_slice(payload);
}

/// A WASI command whose functions are `bodies`, all of type `() -> ()`;
/// the first is `_start`.
fn module(bodies: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"\0asm\x01\0\0\0".to_vec();
    section(1, &[1, 0x60, 0, 0], &mut out);
    let mut functions = Vec::new();
    uleb(bodies.len() as u64, &mut functions);
    functions.resize(functions.len() + bodies.len(), 0);
    section(3, &functions, &mut out);
    section(5, &[1, 0, 1], &mut out);
    let mut exports = vec![2, 6];
    exports.extend_from_slice(b"memory");
    exports.extend_from_slice(&[2, 0, 6]);
    exports.extend_from_slice(b"_start");
    exports.extend_from_slice(&[0, 0]);
    section(7, &exports, &mut out);
    let mut code = Vec::new();
    uleb(bodies.len() as u64, &mut code);
    for body in bodies {
        uleb(body.len() as u64, &mut code);
        code.extend_from_slice(body);
    }
    section(10, &code, &mut out);
    out
}

/// SplitMix64: the same modules on every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// One function body of `kind` (`tiny` or one of [`KINDS`]), at most
/// `max` bytes.
fn body(kind: &str, max: usize, rng: &mut Rng) -> Vec<u8> {
    const LOCALS: u64 = 1000;
    let mut b = Vec::new();
    if kind == "tiny" {
        b.push(0);
    } else {
        b.push(1);
        uleb(LOCALS, &mut b);
        b.push(0x7f); // i32
    }
    let local = |rng: &mut Rng, b: &mut Vec<u8>| uleb(rng.next() % LOCALS, b);
    match kind {
        // i32.const 1; drop, repeated: dead, trivial code.
        "tiny" => {
            while b.len() + 3 < max {
                b.extend_from_slice(&[0x41, 0x01, 0x1a]);
            }
        }
        // local.get a; local.get b; i32.add; local.set c: no branch.
        "arith" => {
            while b.len() + 16 < max {
                b.push(0x20);
                local(rng, &mut b);
                b.push(0x20);
                local(rng, &mut b);
                b.push(0x6a);
                b.push(0x21);
                local(rng, &mut b);
            }
        }
        // One loop around if/else assigning random locals: merges
        // everywhere, long live ranges for the register allocator.
        "branches" => {
            b.extend_from_slice(&[0x03, 0x40]); // loop
            while b.len() + 32 < max {
                b.push(0x20);
                local(rng, &mut b);
                b.extend_from_slice(&[0x04, 0x40]); // if
                b.push(0x20);
                local(rng, &mut b);
                b.push(0x21);
                local(rng, &mut b);
                b.push(0x05); // else
                b.push(0x20);
                local(rng, &mut b);
                b.push(0x21);
                local(rng, &mut b);
                b.push(0x0b); // end
            }
            b.push(0x20);
            local(rng, &mut b);
            b.extend_from_slice(&[0x0d, 0x00]); // br_if 0
            b.push(0x0b); // end loop
        }
        // Blocks nested as deep as the size allows.
        "nest" => {
            let depth = (max - 8) / 3;
            for _ in 0..depth {
                b.extend_from_slice(&[0x02, 0x40]);
            }
            b.resize(b.len() + depth, 0x0b);
        }
        // A br_table with as many targets as fit.
        _ => {
            b.extend_from_slice(&[0x02, 0x40, 0x02, 0x40, 0x41, 0x00, 0x0e]);
            let targets = (max - 32) as u64;
            uleb(targets, &mut b);
            for i in 0..targets {
                b.push((i % 2) as u8);
            }
            b.extend_from_slice(&[0, 0x0b, 0x0b]);
        }
    }
    b.push(0x0b);
    b
}

/// `false` for a variant this bench does not know.
fn compile(variant: &str) -> bool {
    let mut rng = Rng(42);
    let bodies: Vec<Vec<u8>> = if let Some(count) = variant.strip_prefix("many-funcs") {
        let per = (MAX_MODULE - 1_000_000 * 4 - 64) / 1_000_000;
        let count = match count.strip_prefix(':').map(str::parse) {
            None => 1_000_000,
            Some(Ok(n)) => n,
            Some(Err(_)) => return false,
        };
        (0..count)
            .map(|_| body("tiny", per - 4, &mut rng))
            .collect()
    } else if let Some(spec) = variant.strip_prefix("one-") {
        let (kind, size) = match spec.split_once(':') {
            Some((kind, kib)) => match kib.parse::<usize>() {
                Ok(kib) => (kind, kib << 10),
                Err(_) => return false,
            },
            None => (spec, MAX_BODY),
        };
        if !KINDS.contains(&kind) || !(64..=MAX_BODY).contains(&size) {
            return false;
        }
        vec![body(kind, size, &mut rng)]
    } else if KINDS.contains(&variant) {
        let count = MAX_MODULE / (MAX_BODY + 8);
        (0..count)
            .map(|_| body(variant, MAX_BODY, &mut rng))
            .collect()
    } else {
        return false;
    };
    let wasm = module(&bodies);
    println!(
        "compile {variant}: {} functions, {:.2} MiB",
        bodies.len(),
        wasm.len() as f64 / f64::from(1 << 20)
    );
    drop(bodies);
    let host = Host::new().unwrap_or_else(|e| panic!("{e}"));
    let limits = manifest("timeout_ms = 1000\nmemory_mib = 16\nmax_output_mib = 1");
    let sampler = Sampler::start();
    let started = Instant::now();
    let label = match host.load_bytes(limits, &wasm) {
        Ok(_) => format!("  loaded in {:.2} s", started.elapsed().as_secs_f64()),
        Err(e) => format!(
            "  refused after {:.2} s: {}",
            started.elapsed().as_secs_f64(),
            e.to_string().chars().take(160).collect::<String>()
        ),
    };
    sampler.finish(&label);
    true
}

/// What a real module looks like, to compare with a hostile one.
fn inspect(wasm: &[u8]) {
    use wasmparser::{Operator, Parser, Payload};
    let (mut functions, mut largest, mut code) = (0usize, 0usize, 0usize);
    let (mut deepest, mut most_locals, mut imports, mut exports) = (0usize, 0u32, 0u32, 0u32);
    for payload in Parser::new(0).parse_all(wasm) {
        let payload = match payload {
            Ok(payload) => payload,
            Err(e) => {
                println!("not a valid module: {e}");
                return;
            }
        };
        match payload {
            Payload::ImportSection(s) => imports += s.count(),
            Payload::ExportSection(s) => exports += s.count(),
            Payload::CodeSectionEntry(body) => {
                functions += 1;
                let size = body.range().len();
                largest = largest.max(size);
                code += size;
                let locals: u32 = body
                    .get_locals_reader()
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|(count, _)| count)
                    .sum();
                most_locals = most_locals.max(locals);
                let mut depth = 0usize;
                if let Ok(mut reader) = body.get_operators_reader() {
                    while !reader.eof() {
                        match reader.read() {
                            Ok(
                                Operator::Block { .. }
                                | Operator::Loop { .. }
                                | Operator::If { .. }
                                | Operator::TryTable { .. },
                            ) => {
                                depth += 1;
                                deepest = deepest.max(depth);
                            }
                            Ok(Operator::End) => depth = depth.saturating_sub(1),
                            Ok(_) => {}
                            Err(_) => break,
                        }
                    }
                }
            }
            _ => {}
        }
    }
    println!(
        "{} KiB: {functions} functions ({code} bytes of code), largest body {largest} bytes, deepest nesting {deepest}, most locals {most_locals}, {imports} imports, {exports} exports",
        wasm.len() >> 10
    );
}
