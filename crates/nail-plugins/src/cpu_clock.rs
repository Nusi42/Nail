//! The CPU time of the calling thread (ADR 0204). A plug-in's budgets
//! count what its thread computed, not the time it waited for a processor
//! while other programs ran: on a busy machine a well-behaved plug-in is
//! neither stopped nor charged for someone else's load.
#![expect(
    unsafe_code,
    reason = "GetThreadTimes / clock_gettime of the calling thread"
)]

use std::time::Duration;

/// The CPU time (user and kernel) the calling thread has used so far.
/// Windows counts it in clock ticks (about 15.6 ms), Linux and macOS
/// exactly.
#[cfg(windows)]
pub(crate) fn thread_cpu_time() -> Duration {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{GetCurrentThread, GetThreadTimes};

    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
    // SAFETY: GetCurrentThread's pseudo handle is always valid for the
    // calling thread, and the four pointers point at live locals.
    let ok = unsafe {
        GetThreadTimes(
            GetCurrentThread(),
            &raw mut created,
            &raw mut exited,
            &raw mut kernel,
            &raw mut user,
        )
    };
    debug_assert_ne!(ok, 0, "GetThreadTimes of the current thread");
    // FILETIME counts 100 ns units.
    let units = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
    Duration::from_nanos(
        units(kernel)
            .saturating_add(units(user))
            .saturating_mul(100),
    )
}

/// The CPU time (user and kernel) the calling thread has used so far.
#[cfg(unix)]
pub(crate) fn thread_cpu_time() -> Duration {
    let mut now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `now` is a live, writable timespec; the clock id is
    // supported on Linux and macOS (10.12 and later).
    let rc = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &raw mut now) };
    debug_assert_eq!(rc, 0, "clock_gettime(CLOCK_THREAD_CPUTIME_ID)");
    let secs = u64::try_from(now.tv_sec).unwrap_or(0);
    let nanos = u32::try_from(now.tv_nsec).unwrap_or(0);
    Duration::new(secs, nanos)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::thread_cpu_time;

    #[test]
    fn computing_counts_and_waiting_does_not() {
        let start = thread_cpu_time();
        // Wait: no CPU time.
        std::thread::sleep(Duration::from_millis(300));
        let waited = thread_cpu_time().saturating_sub(start);
        assert!(
            waited < Duration::from_millis(100),
            "waiting counted {waited:?}"
        );
        // Compute for at least 300 ms of the thread's own time.
        let before = thread_cpu_time();
        let mut x = 0u64;
        let spin = Instant::now();
        while thread_cpu_time().saturating_sub(before) < Duration::from_millis(300) {
            x = std::hint::black_box(x.wrapping_add(1));
            assert!(
                spin.elapsed() < Duration::from_secs(60),
                "the CPU clock does not move"
            );
        }
        assert!(thread_cpu_time() >= before + Duration::from_millis(300));
    }

    #[test]
    fn each_thread_has_its_own_clock() {
        let here = thread_cpu_time();
        let there = std::thread::spawn(thread_cpu_time).join();
        assert!(
            there.is_ok_and(|t| t < Duration::from_millis(100)),
            "a new thread starts near 0"
        );
        assert!(thread_cpu_time() >= here);
    }
}
