//! The WASI (preview 1) functions a module receives, and the traps that
//! stand in for every other import.
//!
//! Nothing here reaches the operating system: each function works on the
//! store's own buffers. The host provides:
//!
//! | Function | Behaviour |
//! |---|---|
//! | `fd_read` on 0 | the encoded request, then end of file |
//! | `fd_write` on 1 | the answer, up to the output limit: one byte more stops the module |
//! | `fd_write` on 2 | diagnostics, the first 64 KiB kept |
//! | `fd_fdstat_get`, `fd_close`, `fd_seek` on 0 to 2 | a character device that cannot seek |
//! | `fd_prestat_get`, `fd_prestat_dir_name` | `EBADF`: no directory is preopened |
//! | `args_*`, `environ_*` | empty |
//! | `random_get` | a fixed sequence, the same on every run: no entropy |
//! | `sched_yield` | nothing to yield to |
//! | `proc_exit` | ends the run with its code |
//!
//! Any other import — `path_open`, `sock_accept`, `clock_time_get`,
//! `poll_oneoff`, or a function of another namespace — is linked to a trap
//! naming it: the module stops at its first call with
//! [`crate::HostError::CapabilityDenied`]. There is no clock and no
//! `poll_oneoff`: a module has no notion of time and cannot sleep. The
//! fixed `random_get` keeps the standard library's hash maps working (they
//! ask for a seed) without handing out entropy.
//!
//! Every pointer and length comes from the module: each is checked against
//! its memory, and a bad one gives `EFAULT`, never a host panic.

use std::collections::BTreeSet;
use std::ops::Range;
use std::sync::Arc;

use wasmtime::{Caller, Engine, Extern, ExternType, Linker, Memory, Module, ResourceLimiter};

use crate::budget::{Held, Shared};

#[cfg(any(test, feature = "fuzzing"))]
pub mod fuzzing;

/// Namespace of WASI preview 1 imports.
pub(crate) const WASI: &str = "wasi_snapshot_preview1";

/// The WASI functions implemented below; every other import is a trap.
const PROVIDED: [&str; 14] = [
    "args_get",
    "args_sizes_get",
    "environ_get",
    "environ_sizes_get",
    "fd_close",
    "fd_fdstat_get",
    "fd_prestat_dir_name",
    "fd_prestat_get",
    "fd_read",
    "fd_seek",
    "fd_write",
    "proc_exit",
    "random_get",
    "sched_yield",
];

const ERRNO_SUCCESS: i32 = 0;
const ERRNO_BADF: i32 = 8;
const ERRNO_FAULT: i32 = 21;
const ERRNO_INVAL: i32 = 28;
const ERRNO_SPIPE: i32 = 70;

const FILETYPE_CHARACTER_DEVICE: u8 = 2;
const RIGHT_FD_READ: u64 = 1 << 1;
const RIGHT_FD_WRITE: u64 = 1 << 6;

/// Most buffers one `fd_read` or `fd_write` may name (POSIX `IOV_MAX`).
const MAX_IOVS: u32 = 1024;
/// Standard error kept for diagnostics.
const STDERR_LIMIT: usize = 64 << 10;
/// Most elements a table may hold.
const MAX_TABLE_ELEMENTS: usize = 1 << 20;

/// Why the host stopped a module. Carried through Wasmtime's error as the
/// trap's cause, and turned into a [`crate::HostError`] by the sandbox.
#[derive(Debug, thiserror::Error)]
pub(crate) enum Stop {
    #[error("time limit reached")]
    Timeout,
    #[error("memory limit reached")]
    Memory,
    #[error("the host's memory budget is used up")]
    HostMemory,
    #[error("output limit reached")]
    Output,
    #[error("capability not granted: {0}")]
    Denied(String),
    #[error("the module exports no memory")]
    NoMemory,
    #[error("the module exited with code {0}")]
    Exit(i32),
}

fn stop(reason: Stop) -> wasmtime::Error {
    wasmtime::Error::new(reason)
}

/// The store's data: the module's whole world during one run.
pub(crate) struct Sandbox {
    stdin: Vec<u8>,
    stdin_pos: usize,
    pub(crate) stdout: Vec<u8>,
    stdout_limit: usize,
    pub(crate) stderr: Vec<u8>,
    memory_limit: usize,
    fixed_random: u64,
    /// The host's budget taken by memories and tables: freed with the store.
    in_store: Held,
    /// The host's budget taken by the answer: follows it out of the store.
    answer: Held,
}

impl Sandbox {
    pub(crate) fn new(
        stdin: Vec<u8>,
        stdout_limit: usize,
        memory_limit: usize,
        budget: &Arc<Shared>,
    ) -> Sandbox {
        Sandbox {
            stdin,
            stdin_pos: 0,
            stdout: Vec::new(),
            stdout_limit,
            stderr: Vec::new(),
            memory_limit,
            fixed_random: 0,
            in_store: Held::new(budget),
            answer: Held::new(budget),
        }
    }

    /// Standard output, standard error, and the budget the output holds.
    /// The budget of memories and tables is given back here.
    pub(crate) fn finish(self) -> (Vec<u8>, Vec<u8>, Held) {
        (self.stdout, self.stderr, self.answer)
    }

    /// Append to the answer, or stop the module past the limit. The buffer
    /// doubles, but never past the limit (doubling 2 GiB for a 3 GiB limit
    /// would reserve 4), and grows with `try_reserve_exact`: a host short
    /// of memory stops the module instead of aborting.
    fn write_stdout(&mut self, bytes: &[u8]) -> wasmtime::Result<()> {
        let Some(total) = self
            .stdout
            .len()
            .checked_add(bytes.len())
            .filter(|&n| n <= self.stdout_limit)
        else {
            return Err(stop(Stop::Output));
        };
        if total > self.stdout.capacity() {
            let capacity = self
                .stdout
                .capacity()
                .saturating_mul(2)
                .max(total)
                .min(self.stdout_limit);
            if !self.answer.grow(capacity - self.stdout.capacity()) {
                return Err(stop(Stop::HostMemory));
            }
            self.stdout
                .try_reserve_exact(capacity - self.stdout.len())
                .map_err(|_| stop(Stop::Output))?;
        }
        self.stdout.extend_from_slice(bytes);
        Ok(())
    }

    /// Keep the first [`STDERR_LIMIT`] bytes, drop the rest silently.
    fn write_stderr(&mut self, bytes: &[u8]) {
        let room = STDERR_LIMIT.saturating_sub(self.stderr.len());
        let kept = bytes.get(..room.min(bytes.len())).unwrap_or_default();
        if self.stderr.try_reserve(kept.len()).is_ok() {
            self.stderr.extend_from_slice(kept);
        }
    }

    /// SplitMix64 from a zero seed: looks random, is the same every run.
    fn next_fixed(&mut self) -> u64 {
        self.fixed_random = self.fixed_random.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.fixed_random;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

impl ResourceLimiter for Sandbox {
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        if desired > self.memory_limit {
            return Err(stop(Stop::Memory));
        }
        // A growth that then fails keeps its share until the run is over.
        if !self.in_store.grow(desired.saturating_sub(current)) {
            return Err(stop(Stop::HostMemory));
        }
        Ok(true)
    }

    fn table_growing(
        &mut self,
        current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        if desired > MAX_TABLE_ELEMENTS {
            return Err(stop(Stop::Memory));
        }
        // An element is a pointer.
        let bytes = desired
            .saturating_sub(current)
            .saturating_mul(std::mem::size_of::<usize>());
        if !self.in_store.grow(bytes) {
            return Err(stop(Stop::HostMemory));
        }
        Ok(true)
    }

    fn instances(&self) -> usize {
        1
    }

    fn tables(&self) -> usize {
        8
    }

    fn memories(&self) -> usize {
        1
    }
}

/// A linker for `module`: the WASI functions above, and a trap for each of
/// its other function imports. Imports that are not functions are refused.
pub(crate) fn linker(engine: &Engine, module: &Module) -> Result<Linker<Sandbox>, String> {
    let mut linker = Linker::new(engine);
    provide(&mut linker).map_err(|e| format!("{e:#}"))?;
    let mut denied = BTreeSet::new();
    for import in module.imports() {
        let name = format!("{}::{}", import.module(), import.name());
        let ExternType::Func(ty) = import.ty() else {
            return Err(format!(
                "imports `{name}`, which is not a function: the host only provides functions"
            ));
        };
        if import.module() == WASI && PROVIDED.contains(&import.name()) {
            continue;
        }
        if !denied.insert(name.clone()) {
            continue;
        }
        linker
            .func_new(import.module(), import.name(), ty, move |_, _, _| {
                Err(stop(Stop::Denied(name.clone())))
            })
            .map_err(|e| format!("{e:#}"))?;
    }
    Ok(linker)
}

fn provide(linker: &mut Linker<Sandbox>) -> wasmtime::Result<()> {
    linker.func_wrap(
        WASI,
        "args_sizes_get",
        |caller: Caller<'_, Sandbox>, count: i32, size: i32| zero_pair(caller, count, size),
    )?;
    linker.func_wrap(
        WASI,
        "environ_sizes_get",
        |caller: Caller<'_, Sandbox>, count: i32, size: i32| zero_pair(caller, count, size),
    )?;
    // Nothing to copy: the sizes above said zero.
    linker.func_wrap(WASI, "args_get", |_: i32, _: i32| ERRNO_SUCCESS)?;
    linker.func_wrap(WASI, "environ_get", |_: i32, _: i32| ERRNO_SUCCESS)?;
    linker.func_wrap(WASI, "fd_close", |fd: i32| stdio_or_badf(fd, ERRNO_SUCCESS))?;
    linker.func_wrap(
        WASI,
        "fd_seek",
        |fd: i32, _offset: i64, _whence: i32, _new_offset: i32| stdio_or_badf(fd, ERRNO_SPIPE),
    )?;
    // No preopened directory: this is how WASI says "no filesystem".
    linker.func_wrap(WASI, "fd_prestat_get", |_: i32, _: i32| ERRNO_BADF)?;
    linker.func_wrap(WASI, "fd_prestat_dir_name", |_: i32, _: i32, _: i32| {
        ERRNO_BADF
    })?;
    linker.func_wrap(WASI, "sched_yield", || ERRNO_SUCCESS)?;
    linker.func_wrap(WASI, "proc_exit", |code: i32| -> wasmtime::Result<()> {
        Err(stop(Stop::Exit(code)))
    })?;
    linker.func_wrap(WASI, "fd_fdstat_get", fd_fdstat_get)?;
    linker.func_wrap(WASI, "fd_read", fd_read)?;
    linker.func_wrap(WASI, "fd_write", fd_write)?;
    linker.func_wrap(WASI, "random_get", random_get)?;
    Ok(())
}

fn stdio_or_badf(fd: i32, errno: i32) -> i32 {
    if (0..=2).contains(&fd) {
        errno
    } else {
        ERRNO_BADF
    }
}

fn fd_fdstat_get(mut caller: Caller<'_, Sandbox>, fd: i32, buf: i32) -> wasmtime::Result<i32> {
    let rights = match fd {
        0 => RIGHT_FD_READ,
        1 | 2 => RIGHT_FD_WRITE,
        _ => return Ok(ERRNO_BADF),
    };
    let memory = memory(&mut caller)?;
    let data = memory.data_mut(&mut caller);
    // fdstat (24 bytes): filetype u8, flags u16 at 2, rights u64 at 8 and 16.
    let mut stat = [0u8; 24];
    stat[0] = FILETYPE_CHARACTER_DEVICE;
    stat[8..16].copy_from_slice(&rights.to_le_bytes());
    Ok(
        match range(addr(buf), stat.len()).and_then(|r| data.get_mut(r)) {
            Some(dest) => {
                dest.copy_from_slice(&stat);
                ERRNO_SUCCESS
            }
            None => ERRNO_FAULT,
        },
    )
}

fn fd_read(
    mut caller: Caller<'_, Sandbox>,
    fd: i32,
    iovs: i32,
    count: i32,
    nread: i32,
) -> wasmtime::Result<i32> {
    if fd != 0 {
        return Ok(ERRNO_BADF);
    }
    let Some(count) = iov_count(count) else {
        return Ok(ERRNO_INVAL);
    };
    let memory = memory(&mut caller)?;
    let (data, sandbox) = memory.data_and_store_mut(&mut caller);
    let Some(buffers) = iovecs(data, addr(iovs), count) else {
        return Ok(ERRNO_FAULT);
    };
    let mut total = 0usize;
    for (ptr, len) in buffers {
        let remaining = sandbox.stdin.get(sandbox.stdin_pos..).unwrap_or_default();
        let n = remaining.len().min(len);
        let (Some(dest), Some(src)) = (
            range(ptr, n).and_then(|r| data.get_mut(r)),
            remaining.get(..n),
        ) else {
            return Ok(ERRNO_FAULT);
        };
        dest.copy_from_slice(src);
        sandbox.stdin_pos += n;
        total += n;
        if n < len {
            break;
        }
    }
    Ok(put_len(data, addr(nread), total))
}

fn fd_write(
    mut caller: Caller<'_, Sandbox>,
    fd: i32,
    iovs: i32,
    count: i32,
    nwritten: i32,
) -> wasmtime::Result<i32> {
    if fd != 1 && fd != 2 {
        return Ok(ERRNO_BADF);
    }
    let Some(count) = iov_count(count) else {
        return Ok(ERRNO_INVAL);
    };
    let memory = memory(&mut caller)?;
    let (data, sandbox) = memory.data_and_store_mut(&mut caller);
    let Some(buffers) = iovecs(data, addr(iovs), count) else {
        return Ok(ERRNO_FAULT);
    };
    let mut total = 0usize;
    for (ptr, len) in buffers {
        let Some(src) = range(ptr, len).and_then(|r| data.get(r)) else {
            return Ok(ERRNO_FAULT);
        };
        if fd == 1 {
            sandbox.write_stdout(src)?;
        } else {
            sandbox.write_stderr(src);
        }
        total = total.saturating_add(src.len());
    }
    Ok(put_len(data, addr(nwritten), total))
}

fn random_get(mut caller: Caller<'_, Sandbox>, buf: i32, len: i32) -> wasmtime::Result<i32> {
    let memory = memory(&mut caller)?;
    let (data, sandbox) = memory.data_and_store_mut(&mut caller);
    let Ok(len) = usize::try_from(addr(len)) else {
        return Ok(ERRNO_FAULT);
    };
    let Some(dest) = range(addr(buf), len).and_then(|r| data.get_mut(r)) else {
        return Ok(ERRNO_FAULT);
    };
    for chunk in dest.chunks_mut(8) {
        for (byte, value) in chunk.iter_mut().zip(sandbox.next_fixed().to_le_bytes()) {
            *byte = value;
        }
    }
    Ok(ERRNO_SUCCESS)
}

/// Write zero at two addresses: `args_sizes_get`, `environ_sizes_get`.
fn zero_pair(mut caller: Caller<'_, Sandbox>, first: i32, second: i32) -> wasmtime::Result<i32> {
    let memory = memory(&mut caller)?;
    let data = memory.data_mut(&mut caller);
    Ok(
        match (
            write_u32(data, addr(first), 0),
            write_u32(data, addr(second), 0),
        ) {
            (Some(()), Some(())) => ERRNO_SUCCESS,
            _ => ERRNO_FAULT,
        },
    )
}

fn memory(caller: &mut Caller<'_, Sandbox>) -> wasmtime::Result<Memory> {
    match caller.get_export("memory") {
        Some(Extern::Memory(memory)) => Ok(memory),
        _ => Err(stop(Stop::NoMemory)),
    }
}

/// A wasm32 pointer or length: an `i32` read as unsigned.
fn addr(value: i32) -> u32 {
    u32::from_ne_bytes(value.to_ne_bytes())
}

fn iov_count(count: i32) -> Option<u32> {
    u32::try_from(count).ok().filter(|&n| n <= MAX_IOVS)
}

fn range(ptr: u32, len: usize) -> Option<Range<usize>> {
    let start = usize::try_from(ptr).ok()?;
    Some(start..start.checked_add(len)?)
}

fn read_u32(data: &[u8], ptr: u32) -> Option<u32> {
    let bytes = data.get(range(ptr, 4)?)?;
    Some(u32::from_le_bytes(bytes.try_into().ok()?))
}

fn write_u32(data: &mut [u8], ptr: u32, value: u32) -> Option<()> {
    data.get_mut(range(ptr, 4)?)?
        .copy_from_slice(&value.to_le_bytes());
    Some(())
}

/// Store a byte count for the module; `EFAULT` when the address is bad.
fn put_len(data: &mut [u8], ptr: u32, len: usize) -> i32 {
    let len = u32::try_from(len).unwrap_or(u32::MAX);
    match write_u32(data, ptr, len) {
        Some(()) => ERRNO_SUCCESS,
        None => ERRNO_FAULT,
    }
}

/// The `(pointer, length)` pairs of an iovec array, `None` when the array
/// lies outside memory.
fn iovecs(data: &[u8], iovs: u32, count: u32) -> Option<Vec<(u32, usize)>> {
    let mut buffers = Vec::new();
    for i in 0..count {
        let base = iovs.checked_add(i.checked_mul(8)?)?;
        let ptr = read_u32(data, base)?;
        let len = usize::try_from(read_u32(data, base.checked_add(4)?)?).ok()?;
        buffers.push((ptr, len));
    }
    Some(buffers)
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn unlimited() -> Arc<Shared> {
        Shared::new(1, usize::MAX)
    }

    #[test]
    fn the_answer_takes_its_bytes_from_the_host_budget() {
        let budget = Shared::new(1, 1000);
        let mut sandbox = Sandbox::new(Vec::new(), 1 << 20, 0, &budget);
        sandbox.write_stdout(&[1; 600]).unwrap();
        // Growing to 1200 bytes of capacity would pass the budget.
        let err = sandbox.write_stdout(&[2; 600]).unwrap_err();
        assert!(matches!(err.downcast_ref::<Stop>(), Some(Stop::HostMemory)));
        let (stdout, _, held) = sandbox.finish();
        assert_eq!(stdout.len(), 600);
        // The answer holds its share until dropped, then gives it back.
        let mut other = Held::new(&budget);
        assert!(!other.grow(401));
        drop(held);
        assert!(other.grow(1000));
    }

    #[test]
    fn the_answer_buffer_never_reserves_past_its_limit() {
        // Doubling from 600 bytes would reserve 1200 for a limit of 1000:
        // with a limit of 4 GiB, twice that in host memory.
        let mut sandbox = Sandbox::new(Vec::new(), 1000, 0, &unlimited());
        sandbox.write_stdout(&[1; 600]).unwrap();
        sandbox.write_stdout(&[2; 400]).unwrap();
        assert!(
            sandbox.stdout.capacity() <= 1000,
            "{}",
            sandbox.stdout.capacity()
        );
        assert!(sandbox.write_stdout(&[3]).is_err());
        // Many small writes: still within the limit, still amortized.
        let mut sandbox = Sandbox::new(Vec::new(), 100_000, 0, &unlimited());
        for _ in 0..100_000 {
            sandbox.write_stdout(&[0]).unwrap();
        }
        assert!(sandbox.stdout.capacity() <= 100_000);
    }
}
