//! Process memory for the `peak-memory-mib` budget: the peak working set
//! on Windows (`GetProcessMemoryInfo`), `VmHWM` on Linux; `None` elsewhere.

/// Bytes in a MiB.
const MIB: f64 = 1024.0 * 1024.0;

/// The peak working set of this process in MiB, if the platform tells.
pub fn peak_working_set_mib() -> Option<f64> {
    peak_bytes().map(|b| crate::num::f64_of(b) / MIB)
}

/// The current working set of this process in MiB, if the platform tells.
pub fn working_set_mib() -> Option<f64> {
    current_bytes().map(|b| crate::num::f64_of(b) / MIB)
}

#[cfg(windows)]
fn counters() -> Option<windows_sys::Win32::System::ProcessStatus::PROCESS_MEMORY_COUNTERS> {
    #![expect(unsafe_code, reason = "GetProcessMemoryInfo")]
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut counters = PROCESS_MEMORY_COUNTERS::default();
    let size = u32::try_from(size_of::<PROCESS_MEMORY_COUNTERS>()).ok()?;
    counters.cb = size;
    // SAFETY: `GetCurrentProcess` returns a pseudo handle that needs no
    // closing; `counters` is a valid, writable PROCESS_MEMORY_COUNTERS of
    // `size` bytes for the call's duration.
    let ok = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &raw mut counters, size) };
    (ok != 0).then_some(counters)
}

#[cfg(windows)]
fn peak_bytes() -> Option<u64> {
    counters().map(|c| c.PeakWorkingSetSize as u64)
}

#[cfg(windows)]
fn current_bytes() -> Option<u64> {
    counters().map(|c| c.WorkingSetSize as u64)
}

#[cfg(target_os = "linux")]
fn status_kib(field: &str) -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find_map(|l| l.strip_prefix(field))
        .and_then(|rest| {
            rest.trim()
                .trim_end_matches("kB")
                .trim()
                .parse::<u64>()
                .ok()
        })
}

#[cfg(target_os = "linux")]
fn peak_bytes() -> Option<u64> {
    status_kib("VmHWM:").map(|k| k * 1024)
}

#[cfg(target_os = "linux")]
fn current_bytes() -> Option<u64> {
    status_kib("VmRSS:").map(|k| k * 1024)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn peak_bytes() -> Option<u64> {
    None
}

#[cfg(not(any(windows, target_os = "linux")))]
fn current_bytes() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(any(windows, target_os = "linux"))]
    fn the_peak_grows_with_allocations() {
        let before = peak_working_set_mib().unwrap();
        assert!(before > 0.0);
        let block = vec![1u8; 64 * 1024 * 1024];
        std::hint::black_box(&block);
        let now = working_set_mib().unwrap();
        let after = peak_working_set_mib().unwrap();
        assert!(after >= before);
        assert!(after >= now);
        assert!(after > 32.0, "{after}");
        drop(block);
    }
}
