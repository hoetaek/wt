use anyhow::{Context, Result, bail};
use nix::libc::{c_int, c_void, gid_t, uid_t};
use std::mem;

const PROC_PIDTBSDINFO: c_int = 3;
const PROC_PIDTBSDINFO_SIZE: c_int = mem::size_of::<ProcBsdInfo>() as c_int;
const MAXCOMLEN: usize = 16;

#[repr(C)]
#[allow(non_camel_case_types)]
struct ProcBsdInfo {
    pbi_flags: u32,
    pbi_status: u32,
    pbi_xstatus: u32,
    pbi_pid: u32,
    pbi_ppid: u32,
    pbi_uid: uid_t,
    pbi_gid: gid_t,
    pbi_ruid: uid_t,
    pbi_rgid: gid_t,
    pbi_svuid: uid_t,
    pbi_svgid: gid_t,
    rfu_1: u32,
    pbi_comm: [u8; MAXCOMLEN],
    pbi_name: [u8; 2 * MAXCOMLEN],
    pbi_nfiles: u32,
    pbi_pgid: u32,
    pbi_pjobc: u32,
    e_tdev: u32,
    e_tpgid: u32,
    pbi_nice: i32,
    pbi_start_tvsec: u64,
    pbi_start_tvusec: u64,
}

unsafe extern "C" {
    fn proc_pidinfo(
        pid: c_int,
        flavor: c_int,
        arg: u64,
        buffer: *mut c_void,
        buffersize: c_int,
    ) -> c_int;
}

pub fn process_start_time(pid: i32) -> Result<String> {
    let mut info = mem::MaybeUninit::<ProcBsdInfo>::zeroed();
    let size = unsafe {
        proc_pidinfo(
            pid,
            PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast::<c_void>(),
            PROC_PIDTBSDINFO_SIZE,
        )
    };
    if size != PROC_PIDTBSDINFO_SIZE {
        bail!("Failed to read process start time for pid {pid}: proc_pidinfo returned {size}");
    }
    let info = unsafe { info.assume_init() };
    let nanos = info
        .pbi_start_tvusec
        .checked_mul(1_000)
        .context("Process start time microseconds overflowed")?;
    Ok(format!("{}.{nanos:09}", info.pbi_start_tvsec))
}
