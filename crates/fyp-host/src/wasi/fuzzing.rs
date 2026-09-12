//! Fuzzing the WASI functions a module receives.
//!
//! A trampoline module imports the functions of [`PROVIDED`] through the
//! production linker ([`super::linker`]) and exports one wrapper per
//! function, so every call reaches the host with a real `Caller`, as from
//! a module. The input is a script: memory pokes and growth, then calls
//! with arguments chosen to hurt (addresses at and past the end of memory,
//! wrapping around 4 GiB, negative counts, lengths near `u32::MAX`).
//!
//! After every call, [`check`] compares the store with a reference model
//! written from the WASI specification, independently of `wasi.rs`, in
//! 64-bit arithmetic:
//! - the result: errno, exit, output limit, missing memory;
//! - every byte of module memory: a function writes only where it may,
//!   and only the expected bytes;
//! - standard output and error: exactly the bytes of the ranges named,
//!   within their limits;
//! - the standard input position and the fixed random sequence.
//!
//! A host panic escapes [`check`] as a panic; any other divergence is an
//! `Err`. Used by `fuzz/fuzz_targets/host_wasi.rs` and by a randomized test
//! run with `cargo test`.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use wasmtime::{InstancePre, Store, Val, ValType};

use super::{Sandbox, Stop, PROVIDED, STDERR_LIMIT};
use crate::Host;

const BADF: i32 = 8;
const FAULT: i32 = 21;
const INVAL: i32 = 28;
const SPIPE: i32 = 70;
const PAGE: usize = 65536;
/// Longest script run, in operations.
const MAX_OPS: usize = 512;

/// What a call did, as seen by the module.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    Errno(i32),
    Exit(i32),
    OutputLimit,
    NoMemory,
}

impl Outcome {
    fn label(&self) -> String {
        match self {
            Outcome::Errno(n) => n.to_string(),
            Outcome::Exit(_) => "exit".into(),
            Outcome::OutputLimit => "output".into(),
            Outcome::NoMemory => "nomemory".into(),
        }
    }
}

/// `function:outcome` pairs reached by a script, e.g. `fd_read:21`.
pub type Coverage = BTreeSet<String>;

struct Trampolines {
    with_memory: InstancePre<Sandbox>,
    without_memory: InstancePre<Sandbox>,
}

/// Compiled once per process: compiling for each input would make the
/// fuzzer measure Cranelift.
fn trampolines() -> Result<&'static Trampolines, String> {
    static SHARED: OnceLock<Result<Trampolines, String>> = OnceLock::new();
    SHARED
        .get_or_init(|| {
            let host = Host::new().map_err(|e| e.to_string())?;
            let build = |memory: bool| -> Result<InstancePre<Sandbox>, String> {
                let wasm = wat::parse_str(trampoline_wat(memory)).map_err(|e| e.to_string())?;
                let module = wasmtime::Module::from_binary(host.engine(), &wasm)
                    .map_err(|e| format!("{e:#}"))?;
                super::linker(host.engine(), &module)?
                    .instantiate_pre(&module)
                    .map_err(|e| format!("{e:#}"))
            };
            Ok(Trampolines {
                with_memory: build(true)?,
                without_memory: build(false)?,
            })
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// WASI preview 1 signature of each provided function.
fn signature(name: &str) -> (&'static [ValType], bool) {
    use ValType::{I32, I64};
    match name {
        "fd_close" => (&[I32], true),
        "proc_exit" => (&[I32], false),
        "sched_yield" => (&[], true),
        "fd_prestat_dir_name" => (&[I32, I32, I32], true),
        "fd_read" | "fd_write" => (&[I32, I32, I32, I32], true),
        "fd_seek" => (&[I32, I64, I32, I32], true),
        _ => (&[I32, I32], true),
    }
}

fn trampoline_wat(memory: bool) -> String {
    let mut imports = String::new();
    let mut wrappers = String::new();
    for name in PROVIDED {
        let (params, result) = signature(name);
        let types: Vec<&str> = params
            .iter()
            .map(|t| {
                if matches!(t, ValType::I64) {
                    "i64"
                } else {
                    "i32"
                }
            })
            .collect();
        let params = if types.is_empty() {
            String::new()
        } else {
            format!("(param {})", types.join(" "))
        };
        let result = if result { "(result i32)" } else { "" };
        let args: String = (0..types.len())
            .map(|i| format!(" (local.get {i})"))
            .collect();
        imports.push_str(&format!(
            "(import \"wasi_snapshot_preview1\" \"{name}\" (func ${name} {params} {result}))\n"
        ));
        wrappers.push_str(&format!(
            "(func (export \"{name}\") {params} {result} (call ${name}{args}))\n"
        ));
    }
    let memory = if memory {
        "(memory (export \"memory\") 1)"
    } else {
        ""
    };
    format!("(module\n{imports}{memory}\n{wrappers}(func (export \"_start\")))")
}

/// The fuzz input, read as a script. Past its end every byte reads as 0.
struct Script<'a> {
    bytes: &'a [u8],
    pos: usize,
    /// Addresses of the iovecs set up so far: a call's `iovs` often names
    /// one, or the functions would almost never see a valid array.
    recent: Vec<u32>,
}

impl Script<'_> {
    fn done(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    fn byte(&mut self) -> u8 {
        let b = self.bytes.get(self.pos).copied().unwrap_or(0);
        self.pos = self.pos.saturating_add(1);
        b
    }

    fn u32(&mut self) -> u32 {
        u32::from_le_bytes([self.byte(), self.byte(), self.byte(), self.byte()])
    }

    fn u16(&mut self) -> u16 {
        u16::from_le_bytes([self.byte(), self.byte()])
    }

    /// An address or a length, biased towards the edges of `memory_len`
    /// and of the 32-bit address space.
    fn edge(&mut self, memory_len: u32) -> u32 {
        match self.byte() % 10 {
            0..=2 => u32::from(self.byte()),
            3 => memory_len.wrapping_sub(u32::from(self.byte() % 32)),
            4 => memory_len.wrapping_add(u32::from(self.byte() % 32)),
            5 => u32::MAX.wrapping_sub(u32::from(self.byte() % 32)),
            6 => 0x8000_0000u32
                .wrapping_add(u32::from(self.byte() % 16))
                .wrapping_sub(8),
            7 | 8 if !self.recent.is_empty() => {
                let i = usize::from(self.byte()) % self.recent.len();
                self.recent[i]
            }
            _ => self.u32(),
        }
    }

    fn iov_count(&mut self) -> u32 {
        match self.byte() % 6 {
            0..=2 => u32::from(self.byte() % 5),
            3 => 1023 + u32::from(self.byte() % 3),
            4 => u32::MAX.wrapping_sub(u32::from(self.byte() % 4)),
            _ => self.u32(),
        }
    }

    fn fd(&mut self) -> u32 {
        match self.byte() % 4 {
            0..=2 => u32::from(self.byte() % 6).wrapping_sub(1),
            _ => self.u32(),
        }
    }
}

/// Run `input` as a script against the provided WASI functions and the
/// reference model. `Err` names the first divergence.
pub fn check(input: &[u8]) -> Result<Coverage, String> {
    let trampolines = trampolines()?;
    let mut script = Script {
        bytes: input,
        pos: 0,
        recent: Vec::new(),
    };
    let has_memory = !script.byte().is_multiple_of(8);
    let stdin: Vec<u8> = (0..usize::from(script.u16() % 2048))
        .map(|i| (i % 251) as u8)
        .collect();
    let stdout_limit = usize::from(script.u16() % 4096);
    let memory_limit = (1 + usize::from(script.byte() % 4)) * PAGE;

    let mut store = Store::new(
        trampolines.with_memory.module().engine(),
        Sandbox::new(
            stdin,
            stdout_limit,
            memory_limit,
            &crate::budget::Shared::new(1, usize::MAX),
        ),
    );
    store.limiter(|sandbox| sandbox);
    store.set_epoch_deadline(u64::MAX / 2);
    let pre = if has_memory {
        &trampolines.with_memory
    } else {
        &trampolines.without_memory
    };
    let instance = pre
        .instantiate(&mut store)
        .map_err(|e| format!("setup: {e:#}"))?;
    let memory = instance.get_memory(&mut store, "memory");

    let mut coverage = Coverage::new();
    let mut ops = 0;
    while !script.done() && ops < MAX_OPS {
        ops += 1;
        let len = memory.map_or(0, |m| {
            u32::try_from(m.data_size(&store)).unwrap_or(u32::MAX)
        });
        match script.byte() % 8 {
            // Set up iovecs and buffers: host-side writes, not under test.
            0 => {
                let (at, value) = (script.edge(len), script.edge(len));
                poke(memory, &mut store, at, &value.to_le_bytes());
            }
            // A valid iovec: long, or exactly at a buffer's limit, so
            // standard output and error reach their limits byte for byte.
            1 => {
                let at = script.u32() % len.max(1);
                let ptr = script.u32() % len.max(1);
                let sandbox = store.data();
                let room = |limit: usize, used: usize| {
                    u32::try_from(limit.saturating_sub(used)).unwrap_or(u32::MAX)
                };
                let size = match script.byte() % 4 {
                    0 => room(sandbox.stdout_limit, sandbox.stdout.len()),
                    1 => room(STDERR_LIMIT, sandbox.stderr.len()),
                    _ => (len - ptr).saturating_sub(u32::from(script.byte() % 4)),
                }
                .saturating_add(u32::from(script.byte() % 2));
                let mut iovec = ptr.to_le_bytes().to_vec();
                iovec.extend_from_slice(&size.to_le_bytes());
                poke(memory, &mut store, at, &iovec);
                script.recent.push(at);
            }
            2 => {
                if let Some(m) = memory {
                    // Refused past the limit: an error, and the next calls
                    // see the memory unchanged.
                    let _ = m.grow(&mut store, u64::from(script.byte() % 2));
                }
            }
            _ => {
                let name = PROVIDED[usize::from(script.byte()) % PROVIDED.len()];
                let (types, _) = signature(name);
                let args = arguments(name, types, &mut script, len);
                let outcome = call(&instance, &mut store, name, &args)?;
                coverage.insert(format!("{name}:{}", outcome.label()));
                let sandbox = store.data();
                if sandbox.stderr.len() == STDERR_LIMIT {
                    coverage.insert("stderr:full".into());
                }
                if sandbox.stdout_limit > 0 && sandbox.stdout.len() == sandbox.stdout_limit {
                    coverage.insert("stdout:full".into());
                }
            }
        }
    }
    Ok(coverage)
}

/// Host-side write into module memory, skipped when out of bounds.
fn poke(memory: Option<wasmtime::Memory>, store: &mut Store<Sandbox>, at: u32, bytes: &[u8]) {
    if let Some(m) = memory {
        let at = at as usize;
        if let Some(dest) = m
            .data_mut(store)
            .get_mut(at..at.saturating_add(bytes.len()))
        {
            dest.copy_from_slice(bytes);
        }
    }
}

fn arguments(name: &str, types: &[ValType], script: &mut Script<'_>, len: u32) -> Vec<Val> {
    types
        .iter()
        .enumerate()
        .map(|(i, t)| {
            if matches!(t, ValType::I64) {
                return Val::I64(i64::from_le_bytes([(); 8].map(|()| script.byte())));
            }
            let raw = match (name, i) {
                ("fd_read" | "fd_write", 2) => script.iov_count(),
                ("fd_close" | "fd_fdstat_get" | "fd_read" | "fd_write" | "fd_seek", 0)
                | ("fd_prestat_get" | "fd_prestat_dir_name", 0) => script.fd(),
                ("proc_exit", 0) => script.u32(),
                _ => script.edge(len),
            };
            Val::I32(i32::from_le_bytes(raw.to_le_bytes()))
        })
        .collect()
}

/// One call through its wrapper, checked against the model.
fn call(
    instance: &wasmtime::Instance,
    store: &mut Store<Sandbox>,
    name: &str,
    args: &[Val],
) -> Result<Outcome, String> {
    let func = instance
        .get_func(&mut *store, name)
        .ok_or_else(|| format!("setup: no wrapper for {name}"))?;
    let memory = instance.get_memory(&mut *store, "memory");
    let before = memory.map(|m| m.data(&*store).to_vec());
    let sandbox = store.data();
    let mut model = Model {
        memory: before.clone(),
        stdin: sandbox.stdin.clone(),
        stdin_pos: sandbox.stdin_pos,
        stdout: sandbox.stdout.clone(),
        stdout_limit: sandbox.stdout_limit,
        stderr: sandbox.stderr.clone(),
        fixed: sandbox.fixed_random,
    };
    let expected = model.call(name, args);

    let mut results = vec![Val::I32(0); func.ty(&*store).results().len()];
    let actual = match func.call(&mut *store, args, &mut results) {
        Ok(()) => match results.first() {
            Some(Val::I32(errno)) => Outcome::Errno(*errno),
            _ => return Err(format!("{name}{args:?} returned without stopping")),
        },
        Err(error) => match error.downcast_ref::<Stop>() {
            Some(Stop::Exit(code)) => Outcome::Exit(*code),
            Some(Stop::Output) => Outcome::OutputLimit,
            Some(Stop::NoMemory) => Outcome::NoMemory,
            _ => return Err(format!("{name}{args:?}: unexpected error {error:#}")),
        },
    };

    let context = || format!("{name}{args:?}");
    if actual != expected {
        return Err(format!("{}: {actual:?}, model {expected:?}", context()));
    }
    let after = memory.map(|m| m.data(&*store).to_vec());
    if let (Some(after), Some(model_memory)) = (&after, &model.memory) {
        if after.len() != model_memory.len() {
            return Err(format!("{}: memory size changed", context()));
        }
        if let Some(at) = after.iter().zip(model_memory).position(|(a, b)| a != b) {
            return Err(format!(
                "{}: memory differs from the model at {at}: {} instead of {}",
                context(),
                after[at],
                model_memory[at]
            ));
        }
    }
    let sandbox = store.data();
    if sandbox.stdout != model.stdout {
        return Err(format!(
            "{}: standard output differs from the model",
            context()
        ));
    }
    if sandbox.stderr != model.stderr {
        return Err(format!(
            "{}: standard error differs from the model",
            context()
        ));
    }
    if sandbox.stdin_pos != model.stdin_pos {
        return Err(format!(
            "{}: stdin position {}, model {}",
            context(),
            sandbox.stdin_pos,
            model.stdin_pos
        ));
    }
    if sandbox.fixed_random != model.fixed {
        return Err(format!("{}: random sequence differs", context()));
    }
    if sandbox.stdout.len() > sandbox.stdout_limit || sandbox.stderr.len() > STDERR_LIMIT {
        return Err(format!("{}: a buffer is past its limit", context()));
    }
    Ok(actual)
}

/// WASI preview 1 as the host means to provide it, on a copy of the state.
struct Model {
    memory: Option<Vec<u8>>,
    stdin: Vec<u8>,
    stdin_pos: usize,
    stdout: Vec<u8>,
    stdout_limit: usize,
    stderr: Vec<u8>,
    fixed: u64,
}

impl Model {
    fn call(&mut self, name: &str, args: &[Val]) -> Outcome {
        let int = |i: usize| match args.get(i) {
            Some(Val::I32(v)) => *v,
            _ => 0,
        };
        let (a, b, c, d) = (int(0), int(1), int(2), int(3));
        match name {
            "args_get" | "environ_get" | "sched_yield" => Outcome::Errno(0),
            "fd_close" => Outcome::Errno(if (0..=2).contains(&a) { 0 } else { BADF }),
            // `fd_seek` has an i64 second argument: its fd is still first.
            "fd_seek" => Outcome::Errno(if (0..=2).contains(&a) { SPIPE } else { BADF }),
            "fd_prestat_get" | "fd_prestat_dir_name" => Outcome::Errno(BADF),
            "proc_exit" => Outcome::Exit(a),
            "args_sizes_get" | "environ_sizes_get" => self.zero_pair(a, b),
            "fd_fdstat_get" => self.fdstat(a, b),
            "fd_read" => self.read(a, b, c, d),
            "fd_write" => self.write(a, b, c, d),
            "random_get" => self.random(a, b),
            _ => Outcome::Errno(-1),
        }
    }

    fn fits(&self, ptr: i32, len: u64) -> bool {
        let size = self.memory.as_ref().map_or(0, |m| m.len() as u64);
        u64::from(ptr as u32) + len <= size
    }

    fn put(&mut self, ptr: i32, bytes: &[u8]) -> bool {
        if !self.fits(ptr, bytes.len() as u64) {
            return false;
        }
        if let Some(memory) = self.memory.as_mut() {
            let start = ptr as u32 as usize;
            memory[start..start + bytes.len()].copy_from_slice(bytes);
        }
        true
    }

    fn u32_at(&self, ptr: u64) -> Option<u32> {
        let memory = self.memory.as_ref()?;
        if ptr + 4 > memory.len() as u64 {
            return None;
        }
        let at = ptr as usize;
        Some(u32::from_le_bytes([
            memory[at],
            memory[at + 1],
            memory[at + 2],
            memory[at + 3],
        ]))
    }

    fn iovecs(&self, iovs: i32, count: u32) -> Option<Vec<(i32, u64)>> {
        (0..u64::from(count))
            .map(|i| {
                let base = u64::from(iovs as u32) + 8 * i;
                let ptr = self.u32_at(base)?;
                let len = self.u32_at(base + 4)?;
                Some((ptr as i32, u64::from(len)))
            })
            .collect()
    }

    fn zero_pair(&mut self, a: i32, b: i32) -> Outcome {
        if self.memory.is_none() {
            return Outcome::NoMemory;
        }
        let first = self.put(a, &[0; 4]);
        let second = self.put(b, &[0; 4]);
        Outcome::Errno(if first && second { 0 } else { FAULT })
    }

    fn fdstat(&mut self, fd: i32, buf: i32) -> Outcome {
        // Rights (WASI preview 1): fd_read is bit 1, fd_write bit 6.
        let rights: u64 = match fd {
            0 => 1 << 1,
            1 | 2 => 1 << 6,
            _ => return Outcome::Errno(BADF),
        };
        if self.memory.is_none() {
            return Outcome::NoMemory;
        }
        let mut stat = [0u8; 24];
        stat[0] = 2; // filetype character_device
        stat[8..16].copy_from_slice(&rights.to_le_bytes());
        Outcome::Errno(if self.put(buf, &stat) { 0 } else { FAULT })
    }

    fn read(&mut self, fd: i32, iovs: i32, count: i32, nread: i32) -> Outcome {
        if fd != 0 {
            return Outcome::Errno(BADF);
        }
        if !(0..=1024).contains(&count) {
            return Outcome::Errno(INVAL);
        }
        if self.memory.is_none() {
            return Outcome::NoMemory;
        }
        let Some(buffers) = self.iovecs(iovs, count as u32) else {
            return Outcome::Errno(FAULT);
        };
        let mut total = 0u64;
        for (ptr, len) in buffers {
            let left = (self.stdin.len() - self.stdin_pos) as u64;
            let n = left.min(len);
            let chunk = self.stdin[self.stdin_pos..self.stdin_pos + n as usize].to_vec();
            if !self.put(ptr, &chunk) {
                return Outcome::Errno(FAULT);
            }
            self.stdin_pos += n as usize;
            total += n;
            if n < len {
                break;
            }
        }
        let reported = total.min(u64::from(u32::MAX)) as u32;
        Outcome::Errno(if self.put(nread, &reported.to_le_bytes()) {
            0
        } else {
            FAULT
        })
    }

    fn write(&mut self, fd: i32, iovs: i32, count: i32, nwritten: i32) -> Outcome {
        if fd != 1 && fd != 2 {
            return Outcome::Errno(BADF);
        }
        if !(0..=1024).contains(&count) {
            return Outcome::Errno(INVAL);
        }
        if self.memory.is_none() {
            return Outcome::NoMemory;
        }
        let Some(buffers) = self.iovecs(iovs, count as u32) else {
            return Outcome::Errno(FAULT);
        };
        let mut total = 0u64;
        for (ptr, len) in buffers {
            if !self.fits(ptr, len) {
                return Outcome::Errno(FAULT);
            }
            let start = ptr as u32 as usize;
            let bytes = self
                .memory
                .as_ref()
                .map(|m| m[start..start + len as usize].to_vec())
                .unwrap_or_default();
            if fd == 1 {
                if self.stdout.len() as u64 + len > self.stdout_limit as u64 {
                    return Outcome::OutputLimit;
                }
                self.stdout.extend_from_slice(&bytes);
            } else {
                let room = STDERR_LIMIT - self.stderr.len();
                self.stderr
                    .extend_from_slice(&bytes[..room.min(bytes.len())]);
            }
            total += len;
        }
        let reported = total.min(u64::from(u32::MAX)) as u32;
        Outcome::Errno(if self.put(nwritten, &reported.to_le_bytes()) {
            0
        } else {
            FAULT
        })
    }

    fn random(&mut self, buf: i32, len: i32) -> Outcome {
        if self.memory.is_none() {
            return Outcome::NoMemory;
        }
        let len = u64::from(len as u32);
        if !self.fits(buf, len) {
            return Outcome::Errno(FAULT);
        }
        let mut bytes = Vec::with_capacity(len as usize);
        while (bytes.len() as u64) < len {
            // SplitMix64 (Steele, Lea, Flood 2014) from a zero seed.
            self.fixed = self.fixed.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.fixed;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            bytes.extend_from_slice(&(z ^ (z >> 31)).to_le_bytes());
        }
        bytes.truncate(len as usize);
        self.put(buf, &bytes);
        Outcome::Errno(0)
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// Scripts from a fixed-seed generator: the same on every run, so a
    /// failure reproduces. The fuzz target explores further.
    #[test]
    fn random_scripts_agree_with_the_model() {
        let mut state = 0x5EED_u64;
        let mut next = move || {
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        };
        let mut coverage = Coverage::new();
        for round in 0..400 {
            let len = 16 + (next() % 1024) as usize;
            let input: Vec<u8> = (0..len).map(|_| next() as u8).collect();
            match check(&input) {
                Ok(reached) => coverage.extend(reached),
                Err(e) => panic!("round {round}: {e}\ninput: {input:02x?}"),
            }
        }
        // Every path that matters was taken at least once.
        let mut expected: Vec<String> = PROVIDED.iter().map(|f| f.to_string()).collect();
        expected.push("stderr:full".into());
        expected.push("stdout:full".into());
        for (function, outcomes) in [
            ("fd_read", &["0", "8", "21", "28", "nomemory"][..]),
            ("fd_write", &["0", "8", "21", "28", "nomemory", "output"]),
            ("fd_fdstat_get", &["0", "8", "21", "nomemory"]),
            ("random_get", &["0", "21", "nomemory"]),
            ("args_sizes_get", &["0", "21", "nomemory"]),
            ("environ_sizes_get", &["0", "21"]),
            ("fd_close", &["0", "8"]),
            ("fd_seek", &["70", "8"]),
            ("proc_exit", &["exit"]),
        ] {
            for outcome in outcomes {
                expected.push(format!("{function}:{outcome}"));
            }
        }
        for want in expected {
            assert!(
                want.contains(':') && coverage.contains(&want)
                    || !want.contains(':')
                        && coverage.iter().any(|c| c.starts_with(&format!("{want}:"))),
                "never reached {want}; reached {coverage:?}"
            );
        }
    }
}
