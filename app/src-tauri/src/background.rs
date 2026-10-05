//! Test runs keep the app's windows away from the user (ADR 0146).
//!
//! The desktop e2e suites start the real app on the developer's desktop.
//! With `NAIL_TEST_BACKGROUND_WINDOWS=1` — honoured only by `test-overrides`
//! builds ([`crate::shell::test_env`]); `startDesktop` sets it — every
//! window the shell creates (main, pop-outs, hidden print windows) is:
//!
//! - **cloaked** (DWM): Windows still counts it as shown (`IsWindowVisible`,
//!   composition, WebView2 keeps rendering, the shell's `window-visible` and
//!   pop-out reports keep their meaning) but never draws it, and the mouse
//!   reaches the windows below it; positions and sizes stay real, so the
//!   pop-out placement logic is unchanged;
//! - **never activated**: `WS_EX_NOACTIVATE` (`focusable(false)`), not
//!   focused at creation, shown with `SW_SHOWNOACTIVATE`, and no
//!   `set_focus` (tao's may even inject an Alt key press to take the
//!   foreground); a maximize is only remembered (Windows has no
//!   non-activating maximize);
//! - **not on the taskbar**.
//!
//! The shell also shows no OS notifications, opens no file dialog (a pick
//! is cancelled) and no Settings app in this mode. Installer builds never
//! have it.

use std::sync::OnceLock;

use tauri::{Manager, Runtime, WebviewWindow, WebviewWindowBuilder};

/// The test variable that turns the mode on (`1`, `true`, `yes`, `on`).
pub const BACKGROUND_WINDOWS_ENV: &str = "NAIL_TEST_BACKGROUND_WINDOWS";

static ACTIVE: OnceLock<bool> = OnceLock::new();

/// Whether the variable's value asks for background windows.
pub fn wanted(value: Option<&str>) -> bool {
    value.is_some_and(|v| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

/// Decides the mode once at start (later calls are ignored).
pub fn init(active: bool) {
    if ACTIVE.set(active).is_ok() && active {
        tracing::info!("test run: windows stay in the background (cloaked, never activated)");
    }
}

/// Whether windows stay in the background (off unless [`init`] said so).
pub fn active() -> bool {
    ACTIVE.get().copied().unwrap_or(false)
}

/// A new window's builder: unfocused, not activatable and off the taskbar
/// in background mode; unchanged otherwise.
pub fn prepare<R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'_, R, M>,
) -> WebviewWindowBuilder<'_, R, M> {
    if active() {
        builder.focused(false).focusable(false).skip_taskbar(true)
    } else {
        builder
    }
}

/// Cloaks a just-built (still hidden) window in background mode, before it
/// is first shown.
pub fn conceal<R: Runtime>(window: &WebviewWindow<R>) {
    if !active() {
        return;
    }
    let target = window.clone();
    let queued = window.run_on_main_thread(move || {
        #[cfg(windows)]
        match target.hwnd() {
            Ok(hwnd) => {
                if let Err(err) = os::cloak(hwnd.0) {
                    tracing::warn!(%err, "cannot cloak a test window");
                }
            }
            Err(err) => tracing::warn!(%err, "no window handle to cloak"),
        }
        #[cfg(not(windows))]
        drop(target);
    });
    if let Err(err) = queued {
        tracing::warn!(%err, "cannot cloak a test window");
    }
}

/// Shows a window; in background mode without activating it (the OS shows
/// it first with `SW_SHOWNOACTIVATE`, so Tauri's own show — which keeps its
/// state right for a later hide — finds it shown already).
pub fn show<R: Runtime>(window: &WebviewWindow<R>) -> tauri::Result<()> {
    if active() {
        let target = window.clone();
        window.run_on_main_thread(move || {
            #[cfg(windows)]
            if let Ok(hwnd) = target.hwnd() {
                os::show_inactive(hwnd.0);
            }
            #[cfg(not(windows))]
            drop(target);
        })?;
    }
    window.show()
}

/// Brings a window up for the user: shown, restored and focused — in
/// background mode only shown (and restored), never focused.
pub fn bring_up<R: Runtime>(window: &WebviewWindow<R>) -> tauri::Result<()> {
    if active() {
        return show(window);
    }
    window.show()?;
    window.unminimize()?;
    window.set_focus()
}

/// What the desktop e2e tests check about one window (`nail_test_shell
/// "windows"`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent facts Windows reports, not a state"
)]
pub struct WindowFacts {
    /// The Tauri label.
    pub label: String,
    /// Shown as far as Windows is concerned (`IsWindowVisible`).
    pub visible: bool,
    /// Cloaked by DWM: never drawn, no mouse input.
    pub cloaked: bool,
    /// Could be activated (no `WS_EX_NOACTIVATE`).
    pub activatable: bool,
    /// The foreground window.
    pub foreground: bool,
}

impl WindowFacts {
    /// The JSON the test control answers (camelCase like the e2e side).
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "label": self.label,
            "visible": self.visible,
            "cloaked": self.cloaked,
            "activatable": self.activatable,
            "foreground": self.foreground,
            "inBackground": self.in_background(),
        })
    }

    /// Takes no input from the user: cloaked, never activatable, not in
    /// the foreground.
    pub fn in_background(&self) -> bool {
        self.cloaked && !self.activatable && !self.foreground
    }
}

/// The facts of every webview window, by label, and whether the
/// foreground window belongs to this process at all (a file dialog, a
/// message box).
pub fn report<R: Runtime>(app: &tauri::AppHandle<R>) -> serde_json::Value {
    let mut windows: Vec<WindowFacts> = app
        .webview_windows()
        .into_iter()
        .map(|(label, window)| facts(label, &window))
        .collect();
    windows.sort_by(|a, b| a.label.cmp(&b.label));
    serde_json::json!({
        "active": active(),
        "foreground": foreground_is_ours(),
        "windows": windows.iter().map(WindowFacts::to_json).collect::<Vec<_>>(),
    })
}

#[cfg(windows)]
fn facts<R: Runtime>(label: String, window: &WebviewWindow<R>) -> WindowFacts {
    match window.hwnd() {
        Ok(hwnd) => os::facts(label, hwnd.0),
        Err(_) => WindowFacts {
            label,
            visible: false,
            cloaked: false,
            activatable: false,
            foreground: false,
        },
    }
}

#[cfg(not(windows))]
fn facts<R: Runtime>(label: String, window: &WebviewWindow<R>) -> WindowFacts {
    WindowFacts {
        label,
        visible: window.is_visible().unwrap_or(false),
        cloaked: false,
        activatable: true,
        foreground: window.is_focused().unwrap_or(false),
    }
}

fn foreground_is_ours() -> bool {
    #[cfg(windows)]
    {
        os::foreground_is_ours()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
mod os {
    //! The Win32 calls (window handles are Tauri's, converted by pointer:
    //! Tauri may use another `windows` crate version).
    #![expect(unsafe_code, reason = "Win32 calls, each with its SAFETY note")]

    use std::ffi::c_void;

    use windows::Win32::Foundation::{HWND, TRUE};
    use windows::Win32::Graphics::Dwm::{
        DWMWA_CLOAK, DWMWA_CLOAKED, DwmGetWindowAttribute, DwmSetWindowAttribute,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetForegroundWindow, GetWindowLongW, GetWindowThreadProcessId,
        IsWindowVisible, SW_SHOWNOACTIVATE, ShowWindow, WS_EX_NOACTIVATE,
    };

    use super::WindowFacts;

    const BOOL_SIZE: u32 = 4;

    /// Cloaks the window (DWM never draws it; it gets no mouse input).
    pub(super) fn cloak(hwnd: *mut c_void) -> Result<(), String> {
        let on = TRUE;
        // SAFETY: a window handle of this process; the attribute value is
        // a BOOL that lives for the call.
        unsafe { DwmSetWindowAttribute(HWND(hwnd), DWMWA_CLOAK, (&raw const on).cast(), BOOL_SIZE) }
            .map_err(|e| e.to_string())
    }

    /// Shows (and restores) the window without activating it.
    pub(super) fn show_inactive(hwnd: *mut c_void) {
        // SAFETY: a window handle of this process.
        let was_visible = unsafe { ShowWindow(HWND(hwnd), SW_SHOWNOACTIVATE) }.as_bool();
        tracing::debug!(was_visible, "test window shown without activation");
    }

    pub(super) fn facts(label: String, hwnd: *mut c_void) -> WindowFacts {
        let hwnd = HWND(hwnd);
        let mut cloaked = 0u32;
        // SAFETY: a window handle of this process; `cloaked` is a DWORD
        // that lives for the call (DWMWA_CLOAKED answers a DWORD).
        let cloaked = unsafe {
            DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, (&raw mut cloaked).cast(), BOOL_SIZE)
        }
        .is_ok()
            && cloaked != 0;
        // SAFETY: plain queries about a window handle of this process.
        let (visible, ex_style, foreground) = unsafe {
            (
                IsWindowVisible(hwnd).as_bool(),
                GetWindowLongW(hwnd, GWL_EXSTYLE),
                GetForegroundWindow() == hwnd,
            )
        };
        WindowFacts {
            label,
            visible,
            cloaked,
            activatable: u32::from_ne_bytes(ex_style.to_ne_bytes()) & WS_EX_NOACTIVATE.0 == 0,
            foreground,
        }
    }

    pub(super) fn foreground_is_ours() -> bool {
        let mut pid = 0u32;
        // SAFETY: the foreground window may be null (then the answer is 0);
        // `pid` lives for the call.
        unsafe {
            let window = GetForegroundWindow();
            if window.is_invalid() {
                return false;
            }
            GetWindowThreadProcessId(window, Some(&raw mut pid));
        }
        pid == std::process::id()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_variable_turns_the_mode_on() {
        for on in ["1", "true", "TRUE", " yes ", "on"] {
            assert!(wanted(Some(on)), "{on:?}");
        }
        for off in ["", "0", "false", "no", "off", "2", "hidden"] {
            assert!(!wanted(Some(off)), "{off:?}");
        }
        assert!(!wanted(None));
    }

    #[test]
    fn the_mode_is_off_unless_started_with_it() {
        // Unit tests never call `init`: real windows behave as shipped.
        assert!(!active());
    }

    #[test]
    fn a_window_is_in_the_background_only_cloaked_and_never_activatable() {
        let facts = WindowFacts {
            label: "main".into(),
            visible: true,
            cloaked: true,
            activatable: false,
            foreground: false,
        };
        assert!(facts.in_background());
        assert!(
            !WindowFacts {
                cloaked: false,
                ..facts.clone()
            }
            .in_background(),
            "drawn on the user's screen"
        );
        assert!(
            !WindowFacts {
                activatable: true,
                ..facts.clone()
            }
            .in_background(),
            "a click could activate it"
        );
        assert!(
            !WindowFacts {
                foreground: true,
                ..facts
            }
            .in_background(),
            "it took the keyboard"
        );
    }
}
