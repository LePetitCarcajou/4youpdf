//! Where the bench runs: the checkout, the moment, the machine. Every report
//! and every reference timings file records them: times taken on another
//! machine, or with another build, compare with nothing.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

/// The root of the checkout this bench was built from.
pub fn repository_root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(Path::parent)
        .map_or_else(|| manifest.to_path_buf(), Path::to_path_buf)
}

/// `path` relative to `root`, with forward slashes, as the page set and the
/// reports write it; `path` as it is when it is not under `root`.
pub fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// SHA-256 of `bytes`, in lowercase hexadecimal.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// A moment in UTC, to the second.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UtcTime {
    /// Year, e.g. 2026.
    pub year: i64,
    /// Month, 1 to 12.
    pub month: u32,
    /// Day of the month, 1 to 31.
    pub day: u32,
    /// Hour, 0 to 23.
    pub hour: u32,
    /// Minute, 0 to 59.
    pub minute: u32,
    /// Second, 0 to 59.
    pub second: u32,
}

impl UtcTime {
    /// Now, by the system clock; the epoch if the clock is set before it.
    pub fn now() -> UtcTime {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs());
        UtcTime::from_unix(seconds)
    }

    /// `seconds` after 1970-01-01T00:00:00Z.
    pub fn from_unix(seconds: u64) -> UtcTime {
        let days = i64::try_from(seconds / 86_400).unwrap_or(0);
        let (year, month, day) = civil_from_days(days);
        let rest = seconds % 86_400;
        let part = |value: u64| u32::try_from(value).unwrap_or(0);
        UtcTime {
            year,
            month,
            day,
            hour: part(rest / 3600),
            minute: part(rest % 3600 / 60),
            second: part(rest % 60),
        }
    }

    /// `2026-09-13`.
    pub fn date(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// `2026-09-13 21:05 UTC`.
    pub fn display(&self) -> String {
        format!("{} {:02}:{:02} UTC", self.date(), self.hour, self.minute)
    }

    /// `20260913-210533`: a directory name that sorts by date.
    pub fn stamp(&self) -> String {
        format!(
            "{:04}{:02}{:02}-{:02}{:02}{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }
}

/// Year, month and day `days` days after 1970-01-01, in the proleptic
/// Gregorian calendar (Howard Hinnant, `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (
        year,
        u32::try_from(month).unwrap_or(1),
        u32::try_from(day).unwrap_or(1),
    )
}

/// Processor, hardware threads and operating system, in one line.
pub fn machine() -> String {
    let threads = std::thread::available_parallelism().map_or(0, std::num::NonZeroUsize::get);
    let cpu = cpu_name().unwrap_or_else(|| "processeur inconnu".to_string());
    let os = os_version().unwrap_or_else(|| std::env::consts::OS.to_string());
    format!("{cpu}, {threads} fils, {os}, {}", std::env::consts::ARCH)
}

fn cpu_name() -> Option<String> {
    if cfg!(windows) {
        let text = output(Command::new("reg").args([
            "query",
            r"HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0",
            "/v",
            "ProcessorNameString",
        ]))?;
        text.lines()
            .find_map(|line| line.split_once("REG_SZ"))
            .map(|(_, name)| name.trim().to_string())
    } else if cfg!(target_os = "macos") {
        output(Command::new("sysctl").args(["-n", "machdep.cpu.brand_string"]))
    } else {
        let info = std::fs::read_to_string("/proc/cpuinfo").ok()?;
        info.lines()
            .find_map(|line| line.strip_prefix("model name"))
            .and_then(|rest| rest.split_once(':'))
            .map(|(_, name)| name.trim().to_string())
    }
}

fn os_version() -> Option<String> {
    if cfg!(windows) {
        output(Command::new("cmd").args(["/C", "ver"]))
    } else {
        output(Command::new("uname").args(["-sr"]))
    }
}

/// `rustc -V`, with the toolchain the checkout selects
/// (`rust-toolchain.toml`), which Cargo used to build the engines.
pub fn rustc_version(root: &Path) -> String {
    output(Command::new("rustc").arg("-V").current_dir(root))
        .unwrap_or_else(|| "rustc inconnu".to_string())
}

/// The commit checked out, abbreviated, marked when the tree differs from it.
pub fn git_revision(root: &Path) -> String {
    let Some(commit) = output(
        Command::new("git")
            .args(["rev-parse", "--short=12", "HEAD"])
            .current_dir(root),
    ) else {
        return "révision inconnue".to_string();
    };
    let changed = output(
        Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(root),
    )
    .is_some_and(|status| !status.is_empty());
    if changed {
        format!("{commit} (arbre de travail modifié)")
    } else {
        commit
    }
}

/// Trimmed standard output of a command that succeeded.
fn output(command: &mut Command) -> Option<String> {
    let out = command.output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_dates() {
        assert_eq!(UtcTime::from_unix(0).date(), "1970-01-01");
        // 2026-09-13T21:05:33Z
        let t = UtcTime::from_unix(1_789_333_533);
        assert_eq!(t.date(), "2026-09-13");
        assert_eq!(t.display(), "2026-09-13 21:05 UTC");
        assert_eq!(t.stamp(), "20260913-210533");
        // A leap day, and the last second of a year.
        assert_eq!(UtcTime::from_unix(951_782_400).date(), "2000-02-29");
        assert_eq!(UtcTime::from_unix(1_798_761_599).stamp(), "20261231-235959");
    }

    #[test]
    fn hashes_and_paths() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let root = Path::new("/r");
        assert_eq!(
            relative(root, &root.join("tests").join("a.pdf")),
            "tests/a.pdf"
        );
    }
}
