//! Windows registry change notifications for the core's watchers (the
//! light / dark mode and contrast of `appearance`, the time zone of the
//! reminders, R8): auto-reset Win32 events, armed with
//! `RegNotifyChangeKeyValue` and waited for with a stop event and a
//! time-out.
// Win32 calls (registry notifications, events, waits); every block states
// why it is sound.
#![expect(
    unsafe_code,
    reason = "Win32 calls (registry notifications, events, waits)"
)]

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::System::Registry::{REG_NOTIFY_CHANGE_LAST_SET, RegNotifyChangeKeyValue};
use windows_sys::Win32::System::Threading::{CreateEventW, SetEvent, WaitForMultipleObjects};

/// An auto-reset Win32 event, closed on drop.
pub(crate) struct Event(HANDLE);

// SAFETY: an event handle may be used and closed from any thread.
unsafe impl Send for Event {}
// SAFETY: SetEvent / waiting are thread-safe on a shared handle.
unsafe impl Sync for Event {}

impl Event {
    /// A new, unset event; `None` when Windows cannot create one (out of
    /// handles).
    pub(crate) fn new() -> Option<Self> {
        // SAFETY: no security attributes, auto-reset, initially unset,
        // unnamed; a null result is checked.
        let handle = unsafe { CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()) };
        (!handle.is_null()).then_some(Self(handle))
    }

    /// Signals the event.
    pub(crate) fn set(&self) {
        // SAFETY: the handle is a live event owned by `self`.
        unsafe { SetEvent(self.0) };
    }
}

impl Drop for Event {
    fn drop(&mut self) {
        // SAFETY: the handle is owned by `self` and closed once.
        unsafe { CloseHandle(self.0) };
    }
}

/// Asks for one notification on `event` when a value under `key` (or, with
/// `subtree`, under its subkeys) changes; `false` when that is not
/// possible.
pub(crate) fn arm(key: &windows_registry::Key, event: &Event, subtree: bool) -> bool {
    // SAFETY: the key and the event are live for the call; the
    // notification is asynchronous and tied to the calling (watcher)
    // thread, which outlives every wait on the event.
    let status = unsafe {
        RegNotifyChangeKeyValue(
            key.as_raw(),
            i32::from(subtree),
            REG_NOTIFY_CHANGE_LAST_SET,
            event.0,
            1,
        )
    };
    status == 0
}

/// Waits until one of `events` is signalled (its index) or `timeout_ms`
/// passes (`None`).
pub(crate) fn wait_any(events: &[&Event], timeout_ms: u32) -> Option<usize> {
    let handles: Vec<HANDLE> = events.iter().map(|e| e.0).collect();
    let count = u32::try_from(handles.len()).ok()?;
    // SAFETY: live event handles; the array outlives the call.
    let woke = unsafe { WaitForMultipleObjects(count, handles.as_ptr(), 0, timeout_ms) };
    let index = woke.checked_sub(WAIT_OBJECT_0)?;
    usize::try_from(index).ok().filter(|i| *i < handles.len())
}
