use anyhow::{Context, Result, bail};
use nix::libc::{_SC_CLK_TCK, sysconf};
use std::fs;

pub fn process_start_time(pid: i32) -> Result<String> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat"))
        .with_context(|| format!("Failed to read /proc/{pid}/stat"))?;
    let start_ticks = proc_stat_start_ticks(&stat)?;
    // SAFETY: sysconf reads process configuration for a constant key and does not
    // require any pointer or ownership invariants from Rust.
    let ticks_per_second = unsafe { sysconf(_SC_CLK_TCK) };
    if ticks_per_second <= 0 {
        bail!("Failed to read _SC_CLK_TCK");
    }
    let ticks_per_second = ticks_per_second as u64;
    let boot_time = linux_boot_time()?;
    let seconds = boot_time + (start_ticks / ticks_per_second);
    let nanos = ((start_ticks % ticks_per_second) * 1_000_000_000) / ticks_per_second;
    Ok(format!("{seconds}.{nanos:09}"))
}

fn proc_stat_start_ticks(stat: &str) -> Result<u64> {
    let end = stat
        .rfind(')')
        .context("Failed to parse /proc stat: missing command terminator")?;
    let after_command = stat
        .get(end + 2..)
        .context("Failed to parse /proc stat fields")?;
    let fields = after_command.split_whitespace().collect::<Vec<_>>();
    let Some(start_time) = fields.get(19) else {
        bail!("Failed to parse /proc stat: missing start_time field");
    };
    start_time
        .parse::<u64>()
        .context("Failed to parse /proc stat start_time field")
}

fn linux_boot_time() -> Result<u64> {
    let stat = fs::read_to_string("/proc/stat").context("Failed to read /proc/stat")?;
    for line in stat.lines() {
        if let Some(value) = line.strip_prefix("btime ") {
            return value
                .trim()
                .parse::<u64>()
                .context("Failed to parse /proc/stat btime");
        }
    }
    bail!("Failed to find btime in /proc/stat")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_proc_stat_start_ticks_with_spaces_in_command() {
        let stat = "123 (cmd with spaces) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 123456 21";
        assert_eq!(proc_stat_start_ticks(stat).unwrap(), 123456);
    }
}
