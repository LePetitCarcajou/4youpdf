//! How much memory the worker holds, as the window reads it to keep it
//! under its ceiling (ADR 0008, « Le gardien de la mémoire »): through the
//! system, without a word from the worker, which is not trusted to tell.
//!
//! - Windows: the commit charge of the process, what a job memory limit
//!   would count (`PagefileUsage` of `GetProcessMemoryInfo`).
//! - Linux: `RssAnon` plus `VmSwap` of `/proc/<pid>/status`, the private
//!   memory of the process whether it is in RAM or swapped out. `VmRSS`
//!   alone would count the pages of the PDFium library, which cost
//!   nothing, and would miss what was swapped out, where a page that only
//!   allocates could then grow without being seen.
//! - elsewhere: nothing, and the worker has no ceiling
//!   (`docs/backlog-technique.md`).

use std::process::Child;

/// The bytes `child` holds, or `None` when the system does not tell.
#[cfg(windows)]
pub(super) fn held(child: &Child) -> Option<u64> {
    use std::os::windows::io::AsRawHandle;
    let counters = win32job::utils::get_process_memory_info(child.as_raw_handle() as isize).ok()?;
    u64::try_from(counters.pagefile_usage).ok()
}

/// The bytes `child` holds, or `None` when the system does not tell.
#[cfg(target_os = "linux")]
pub(super) fn held(child: &Child) -> Option<u64> {
    let status = std::fs::read_to_string(format!("/proc/{}/status", child.id())).ok()?;
    parse_status(&status)
}

/// The bytes `child` holds: not told on this system.
#[cfg(not(any(windows, target_os = "linux")))]
pub(super) fn held(_child: &Child) -> Option<u64> {
    None
}

/// The private memory, in bytes, that a `/proc/<pid>/status` tells:
/// `RssAnon` (`VmRSS` on a kernel older than 4.5, which has no `RssAnon`)
/// plus `VmSwap`. `None` when it tells none of them, as for a process that
/// has ended and waits to be reaped.
#[cfg(any(target_os = "linux", test))]
pub(super) fn parse_status(status: &str) -> Option<u64> {
    let field = |name: &str| {
        status.lines().find_map(|line| {
            let rest = line.strip_prefix(name)?.strip_prefix(':')?;
            let kibibytes: u64 = rest.trim().strip_suffix("kB")?.trim().parse().ok()?;
            kibibytes.checked_mul(1024)
        })
    };
    let resident = field("RssAnon").or_else(|| field("VmRSS"))?;
    Some(resident.saturating_add(field("VmSwap").unwrap_or(0)))
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const STATUS: &str = "Name:\tfyp-app\nUmask:\t0022\nState:\tS (sleeping)\nPid:\t4242\n\
VmPeak:\t  300000 kB\nVmSize:\t  250000 kB\nVmRSS:\t   20480 kB\nRssAnon:\t   10240 kB\n\
RssFile:\t   10000 kB\nRssShmem:\t     240 kB\nVmData:\t   90000 kB\nVmSwap:\t    2048 kB\n\
Threads:\t1\n";

    /// What counts is the private memory, in RAM or swapped out: not the
    /// pages of the files the process maps.
    #[test]
    fn the_private_memory_is_anonymous_pages_plus_swap() {
        assert_eq!(parse_status(STATUS), Some((10240 + 2048) * 1024));
    }

    /// A kernel without `RssAnon` still gives a figure, a larger one.
    #[test]
    fn an_old_kernel_falls_back_on_the_resident_set() {
        let old: String = STATUS
            .lines()
            .filter(|line| !line.starts_with("Rss"))
            .map(|line| format!("{line}\n"))
            .collect();
        assert_eq!(parse_status(&old), Some((20480 + 2048) * 1024));
        let no_swap = old.replace("VmSwap:\t    2048 kB\n", "");
        assert_eq!(parse_status(&no_swap), Some(20480 * 1024));
    }

    /// A process that has ended has no memory lines; a status that is not
    /// one, or a figure that does not fit, tells nothing either. Never a
    /// panic.
    #[test]
    fn a_status_without_memory_tells_nothing() {
        assert_eq!(parse_status("Name:\tfyp-app\nState:\tZ (zombie)\n"), None);
        assert_eq!(parse_status(""), None);
        assert_eq!(parse_status("RssAnon:\tmany kB\n"), None);
        assert_eq!(parse_status("RssAnon:\t12\n"), None);
        assert_eq!(parse_status("RssAnonymous:\t12 kB\n"), None);
        assert_eq!(parse_status("RssAnon:\t18446744073709551615 kB\n"), None);
        assert_eq!(
            parse_status("RssAnon:\t18014398509481983 kB\nVmSwap:\t18014398509481983 kB\n"),
            Some(u64::MAX)
        );
    }
}
