//! M5a appearance (ADR 0102, WP152): the theme list (built-ins +
//! `<config-dir>/themes/`), the selection and overrides in the user's
//! `theme.kdl`, Follow Windows and `ThemeChanged`.
//!
//! The rules and the state live in [`nail_config::ConfigManager`]
//! (`appearance`, `set_theme`, `set_theme_preferences`,
//! `set_system_appearance`, `reload_themes`); this module serves the
//! requests and connects the OS light / dark state:
//!
//! * [`handle`]: `ListThemes`, `SetTheme`, `SetThemePreferences`, all
//!   answered with `Response::Themes`; every resulting config update is
//!   broadcast (`ThemeChanged`, then `ConfigChanged` when the resolved theme
//!   changed; `ConfigErrors` for user theme files).
//! * [`watch_system_appearance`]: subscribes to a
//!   [`SystemAppearanceSource`] (the one in `ServiceOptions::system_appearance`,
//!   else [`platform_source`]: the Windows registry watcher) and pushes
//!   every change into the selection; the returned guard stops it.
//! * [`report_system_appearance`]: the same for a state reported from
//!   outside (the UI's `prefers-color-scheme`, a desktop-shell event).
//! * [`WindowsSystemAppearance`] (Windows): `AppsUseLightTheme` under
//!   `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize`
//!   and `SPI_GETHIGHCONTRAST`, watched with `RegNotifyChangeKeyValue` on
//!   that key and on `HKCU\Control Panel\Accessibility\HighContrast`.
//!
//! `Service::open` starts [`watch_system_appearance`] (WP155) and keeps the
//! guard; `ReportSystemAppearance` reaches [`report_system_appearance`] only
//! while there is no such watch (a core without an OS source), otherwise
//! the report is ignored and the current appearance is answered.
//!
//! Remote sessions (M5b, ADR 0109) follow their own browser: their
//! `ReportSystemAppearance` is kept per session
//! ([`Service::remember_session_appearance`]) and never changes the host;
//! `GetConfig`, `ListThemes` / `SetTheme` / `SetThemePreferences` answers
//! and the `ThemeChanged` / `ConfigChanged` events a session gets are
//! computed for its state ([`Service::session_appearance`],
//! `remote_access.rs`).

use std::sync::Arc;

use nail_api::appearance::{Appearance, SystemAppearance, SystemAppearanceSource};
use nail_api::{ApiError, Event, Request, Response};
use nail_config::{ConfigManager, ConfigUpdate};
use tokio::sync::broadcast;

use crate::error::Result;
use crate::service::Service;

#[cfg(windows)]
pub(crate) use windows::WindowsSystemAppearance;

fn send_all(events: &broadcast::Sender<Event>, updates: &[ConfigUpdate]) {
    for update in updates {
        drop(events.send(update.to_event()));
    }
}

/// `ListThemes`, `SetTheme`, `SetThemePreferences`, `ReportSystemAppearance`.
pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    if let Request::ReportSystemAppearance { system } = request {
        let watching = crate::service::lock(&service.inner.appearance_watch).is_some();
        let appearance = if watching {
            // The OS source wins; the page's view is only a fallback.
            service.config().appearance()
        } else {
            report_system_appearance(service, Some(system))
        };
        return Ok(Response::Themes {
            appearance: Box::new(appearance),
        });
    }
    let config = service.config().clone();
    let events = service.inner.events.clone();
    // The config manager reads and writes files.
    tokio::task::spawn_blocking(move || handle_blocking(&config, &events, request)).await?
}

fn handle_blocking(
    config: &ConfigManager,
    events: &broadcast::Sender<Event>,
    request: Request,
) -> Result<Response> {
    let appearance = match request {
        Request::ListThemes => {
            // Without a watcher (tests, `--no-watch`) the list would go
            // stale; re-reading `themes/` is cheap and reports nothing when
            // nothing changed.
            send_all(events, &config.reload_themes());
            config.appearance()
        }
        Request::SetTheme { theme_id } => {
            let (appearance, updates) = config.set_theme(&theme_id)?;
            send_all(events, &updates);
            appearance
        }
        Request::SetThemePreferences { preferences } => {
            let (appearance, updates) = config.set_theme_preferences(&preferences)?;
            send_all(events, &updates);
            appearance
        }
        other => {
            return Err(ApiError::internal(format!(
                "{} is not an appearance request",
                crate::service::request_name(&other)
            ))
            .into());
        }
    };
    Ok(Response::Themes {
        appearance: Box::new(appearance),
    })
}

/// Applies an OS light / dark state reported from outside the core (the
/// UI's `prefers-color-scheme`, a desktop-shell notification) and
/// broadcasts the result; returns the new [`Appearance`]. `None` = unknown
/// (Follow Windows then uses the light theme).
pub(crate) fn report_system_appearance(
    service: &Service,
    system: Option<SystemAppearance>,
) -> Appearance {
    let config = service.config();
    send_all(&service.inner.events, &config.set_system_appearance(system));
    config.appearance()
}

/// Remote sessions whose reported light / dark state is kept (M5b); when a
/// new session would exceed it, the remembered states are dropped (those
/// sessions fall back to the host's state until they report again).
pub(crate) const SESSION_APPEARANCE_LIMIT: usize = 1024;

impl Service {
    /// Remembers what a remote session's browser reported (M5b, ADR 0109).
    pub(crate) fn remember_session_appearance(&self, session: &str, system: SystemAppearance) {
        let mut sessions = crate::service::lock(&self.inner.session_appearance);
        if !sessions.contains_key(session) && sessions.len() >= SESSION_APPEARANCE_LIMIT {
            sessions.clear();
        }
        sessions.insert(session.to_owned(), system);
    }

    /// The appearance and resolved theme as a remote session sees them: the
    /// active theme follows the session's own reported light / dark state
    /// (else the host's), `Appearance::system` is the session's report
    /// (`None` until it reported, so its UI reports at start).
    pub(crate) fn session_appearance(
        &self,
        session: &str,
    ) -> (Appearance, Arc<nail_api::ResolvedTheme>) {
        let own = crate::service::lock(&self.inner.session_appearance)
            .get(session)
            .copied();
        let config = self.config();
        let effective = own.or_else(|| config.system_appearance());
        let (mut appearance, theme) = config.appearance_for(effective);
        appearance.system = own;
        (appearance, theme)
    }
}

/// The platform's source of the OS light / dark state: the registry
/// watcher on Windows, nothing elsewhere (the UI reports
/// `prefers-color-scheme` through [`report_system_appearance`] instead).
pub(crate) fn platform_source() -> Option<Arc<dyn SystemAppearanceSource>> {
    #[cfg(windows)]
    {
        WindowsSystemAppearance::new()
            .map(|source| -> Arc<dyn SystemAppearanceSource> { Arc::new(source) })
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Keeps a [`SystemAppearanceSource`] subscribed; dropping it unsubscribes
/// (and, for the Windows source, stops its watcher thread once the last
/// reference is gone).
pub(crate) struct SystemAppearanceWatch {
    source: Arc<dyn SystemAppearanceSource>,
    handle: u64,
}

impl std::fmt::Debug for SystemAppearanceWatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemAppearanceWatch")
            .field("source", &self.source)
            .field("handle", &self.handle)
            .finish()
    }
}

impl Drop for SystemAppearanceWatch {
    fn drop(&mut self) {
        self.source.unwatch(self.handle);
    }
}

/// Starts Follow Windows: subscribes to `source` (`None` =
/// [`platform_source`]), applies its current state and pushes every change
/// into the selection, broadcasting `ThemeChanged` / `ConfigChanged`.
/// Returns `None` when there is no source (not Windows and none given).
///
/// The listener holds the config manager and the event sender, not the
/// service, so storing the guard in the service makes no reference cycle.
pub(crate) fn watch_system_appearance(
    service: &Service,
    source: Option<Arc<dyn SystemAppearanceSource>>,
) -> Option<SystemAppearanceWatch> {
    let source = source.or_else(platform_source)?;
    let config = service.config().clone();
    let events = service.inner.events.clone();
    let handle = source.watch(Arc::new(move |system| {
        send_all(&events, &config.set_system_appearance(Some(system)));
    }));
    // After subscribing, so a change in between is not lost.
    let current = source.current();
    send_all(
        &service.inner.events,
        &service.config().set_system_appearance(current),
    );
    Some(SystemAppearanceWatch { source, handle })
}

#[cfg(windows)]
mod windows {
    //! The Windows source of the OS light / dark and high-contrast state.
    // Win32 calls (registry notifications, events, SystemParametersInfoW);
    // every block states why it is sound.
    #![expect(
        unsafe_code,
        reason = "Win32 calls (registry notifications, events, SystemParametersInfoW)"
    )]

    use std::sync::{Arc, Mutex, PoisonError};
    use std::thread::JoinHandle;

    use nail_api::appearance::{
        ColorScheme, SystemAppearance, SystemAppearanceListener, SystemAppearanceSource,
    };
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Registry::{
        REG_NOTIFY_CHANGE_LAST_SET, RegNotifyChangeKeyValue,
    };
    use windows_sys::Win32::System::Threading::{CreateEventW, SetEvent, WaitForMultipleObjects};
    use windows_sys::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
    use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_GETHIGHCONTRAST, SystemParametersInfoW};

    /// The key holding the app light / dark mode.
    pub(super) const PERSONALIZE_KEY: &str =
        r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
    /// The value: 1 = light apps, 0 = dark apps.
    pub(super) const APPS_USE_LIGHT_THEME: &str = "AppsUseLightTheme";
    /// The key Windows updates when a contrast theme is switched on or off.
    const HIGH_CONTRAST_KEY: &str = r"Control Panel\Accessibility\HighContrast";
    /// Re-check this often even without a notification (a missed
    /// notification, or a key that did not exist when the watch started).
    const POLL_MS: u32 = 5_000;

    /// An auto-reset Win32 event, closed on drop.
    struct Event(HANDLE);

    // SAFETY: an event handle may be used and closed from any thread.
    unsafe impl Send for Event {}
    // SAFETY: SetEvent / waiting are thread-safe on a shared handle.
    unsafe impl Sync for Event {}

    impl Event {
        fn new() -> Option<Self> {
            // SAFETY: no security attributes, auto-reset, initially unset,
            // unnamed; a null result is checked.
            let handle = unsafe { CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()) };
            (!handle.is_null()).then_some(Self(handle))
        }

        fn set(&self) {
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

    #[derive(Default)]
    struct Listeners {
        next: u64,
        all: Vec<(u64, SystemAppearanceListener)>,
    }

    struct Shared {
        listeners: Mutex<Listeners>,
        stop: Event,
    }

    /// Reads the OS state from the registry and `SystemParametersInfoW`
    /// and, while anyone watches, notices changes on a background thread.
    pub(crate) struct WindowsSystemAppearance {
        shared: Arc<Shared>,
        thread: Mutex<Option<JoinHandle<()>>>,
    }

    impl std::fmt::Debug for WindowsSystemAppearance {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let listeners = self
                .shared
                .listeners
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .all
                .len();
            f.debug_struct("WindowsSystemAppearance")
                .field("listeners", &listeners)
                .finish_non_exhaustive()
        }
    }

    impl WindowsSystemAppearance {
        /// A source; the watcher thread starts with the first `watch`.
        /// `None` when Windows cannot create an event object (out of
        /// handles).
        pub(crate) fn new() -> Option<Self> {
            Some(Self {
                shared: Arc::new(Shared {
                    listeners: Mutex::new(Listeners::default()),
                    stop: Event::new()?,
                }),
                thread: Mutex::new(None),
            })
        }
    }

    /// The current state; `None` when the personalization key cannot be
    /// read at all (then nothing is known about the colour scheme).
    pub(super) fn read_current() -> Option<SystemAppearance> {
        let key = windows_registry::CURRENT_USER.open(PERSONALIZE_KEY);
        let scheme = match key.and_then(|k| k.get_u32(APPS_USE_LIGHT_THEME)) {
            Ok(0) => ColorScheme::Dark,
            Ok(_) => ColorScheme::Light,
            // Windows versions without the setting are light.
            Err(_) if high_contrast().is_some() => ColorScheme::Light,
            Err(_) => return None,
        };
        Some(SystemAppearance {
            scheme,
            high_contrast: high_contrast().unwrap_or(false),
        })
    }

    /// Whether a contrast theme is on (`None` when Windows does not say).
    fn high_contrast() -> Option<bool> {
        let mut hc = HIGHCONTRASTW {
            cbSize: u32::try_from(size_of::<HIGHCONTRASTW>()).ok()?,
            ..HIGHCONTRASTW::default()
        };
        // SAFETY: `hc` is a properly sized HIGHCONTRASTW that lives for the
        // call; no profile update is requested.
        let ok = unsafe {
            SystemParametersInfoW(
                SPI_GETHIGHCONTRAST,
                hc.cbSize,
                std::ptr::from_mut(&mut hc).cast(),
                0,
            )
        };
        (ok != 0).then_some(hc.dwFlags & HCF_HIGHCONTRASTON != 0)
    }

    /// Asks for one notification on `event` when a value under `key`
    /// changes; `false` when that is not possible.
    fn arm(key: &windows_registry::Key, event: &Event) -> bool {
        // SAFETY: the key and the event are live for the call; the
        // notification is asynchronous and tied to this (the watcher)
        // thread, which outlives every wait on the event.
        let status = unsafe {
            RegNotifyChangeKeyValue(key.as_raw(), 0, REG_NOTIFY_CHANGE_LAST_SET, event.0, 1)
        };
        status == 0
    }

    fn run(shared: &Shared) {
        let personalize = windows_registry::CURRENT_USER.open(PERSONALIZE_KEY).ok();
        let contrast = windows_registry::CURRENT_USER.open(HIGH_CONTRAST_KEY).ok();
        let (Some(personalize_changed), Some(contrast_changed)) = (Event::new(), Event::new())
        else {
            tracing::warn!("cannot create events; Follow Windows does not notice changes");
            return;
        };
        let mut last = read_current();
        loop {
            if let Some(key) = &personalize {
                arm(key, &personalize_changed);
            }
            if let Some(key) = &contrast {
                arm(key, &contrast_changed);
            }
            let handles = [shared.stop.0, personalize_changed.0, contrast_changed.0];
            // SAFETY: three live event handles; the array outlives the call.
            let woke = unsafe { WaitForMultipleObjects(3, handles.as_ptr(), 0, POLL_MS) };
            if woke == WAIT_OBJECT_0 {
                return;
            }
            let now = read_current();
            if now == last {
                continue;
            }
            last = now;
            let Some(state) = now else { continue };
            let listeners: Vec<SystemAppearanceListener> = shared
                .listeners
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .all
                .iter()
                .map(|(_, l)| l.clone())
                .collect();
            for listener in listeners {
                listener(state);
            }
        }
    }

    impl SystemAppearanceSource for WindowsSystemAppearance {
        fn current(&self) -> Option<SystemAppearance> {
            read_current()
        }

        fn watch(&self, listener: SystemAppearanceListener) -> u64 {
            let handle = {
                let mut listeners = self
                    .shared
                    .listeners
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                listeners.next += 1;
                let handle = listeners.next;
                listeners.all.push((handle, listener));
                handle
            };
            let mut thread = self.thread.lock().unwrap_or_else(PoisonError::into_inner);
            if thread.is_none() {
                let shared = Arc::clone(&self.shared);
                match std::thread::Builder::new()
                    .name("nail-system-appearance".into())
                    .spawn(move || run(&shared))
                {
                    Ok(join) => *thread = Some(join),
                    Err(err) => tracing::warn!(%err, "cannot watch the Windows colour mode"),
                }
            }
            handle
        }

        fn unwatch(&self, handle: u64) {
            self.shared
                .listeners
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .all
                .retain(|(h, _)| *h != handle);
        }
    }

    impl Drop for WindowsSystemAppearance {
        fn drop(&mut self) {
            self.shared.stop.set();
            let thread = self
                .thread
                .get_mut()
                .unwrap_or_else(PoisonError::into_inner)
                .take();
            if let Some(thread) = thread {
                drop(thread.join());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::ErrorCode;
    use nail_api::appearance::{
        ColorScheme, Density, ManualSystemAppearance, ThemeOverrides, ThemePreferences,
    };

    struct Fixture {
        service: Service,
        config_dir: tempfile::TempDir,
        _data: tempfile::TempDir,
    }

    fn open() -> Fixture {
        let data = tempfile::tempdir().unwrap();
        let config_dir = tempfile::tempdir().unwrap();
        let mut options = crate::ServiceOptions::new(data.path(), config_dir.path());
        options.watch_config = false;
        options.start_engines = false;
        let service = Service::open(crate::demo::demo_options(options)).unwrap();
        Fixture {
            service,
            config_dir,
            _data: data,
        }
    }

    fn themes(response: Response) -> Appearance {
        match response {
            Response::Themes { appearance } => *appearance,
            other => panic!("expected Themes, got {other:?}"),
        }
    }

    fn drain(rx: &mut broadcast::Receiver<Event>) -> Vec<Event> {
        let mut out = Vec::new();
        while let Ok(event) = rx.try_recv() {
            out.push(event);
        }
        out
    }

    #[tokio::test]
    async fn list_themes_reads_the_themes_directory() {
        let f = open();
        let appearance = themes(f.service.handle(Request::ListThemes).await);
        assert_eq!(appearance.themes.len(), 5);
        assert_eq!(appearance.active, "frost");

        let dir = f.config_dir.path().join("themes");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("ocean.kdl"),
            "theme \"Ocean\" extends=\"graphite\" { tokens { color \"accent\" \"#00aaff\"; }; }",
        )
        .unwrap();
        let mut rx = f.service.subscribe();
        let appearance = themes(f.service.handle(Request::ListThemes).await);
        assert!(appearance.themes.iter().any(|t| t.id == "ocean"));
        let events = drain(&mut rx);
        assert!(
            events.iter().any(
                |e| matches!(e, Event::ThemeChanged { appearance } if appearance.themes.len() == 6)
            ),
            "{events:?}"
        );
    }

    #[tokio::test]
    async fn set_theme_writes_theme_kdl_and_broadcasts() {
        let f = open();
        let mut rx = f.service.subscribe();
        let appearance = themes(
            f.service
                .handle(Request::SetTheme {
                    theme_id: "nocturne".into(),
                })
                .await,
        );
        assert_eq!(appearance.active, "nocturne");
        assert!(!appearance.settings.follow_system);
        let text = std::fs::read_to_string(f.config_dir.path().join("theme.kdl")).unwrap();
        assert!(text.contains("extends=\"nocturne\""), "{text}");
        let events = drain(&mut rx);
        assert!(
            matches!(
                events.as_slice(),
                [Event::ThemeChanged { .. }, Event::ConfigChanged { .. }]
            ),
            "{events:?}"
        );
        assert_eq!(f.service.config().theme().mode, nail_api::ThemeMode::Dark);

        let Response::Error {
            code, localized, ..
        } = f
            .service
            .handle(Request::SetTheme {
                theme_id: "nope".into(),
            })
            .await
        else {
            panic!("expected an error");
        };
        assert_eq!(code, ErrorCode::InvalidRequest);
        assert_eq!(localized.unwrap().id, "theme-problem-unknown");
    }

    #[tokio::test]
    async fn set_theme_preferences_writes_density_and_overrides() {
        let f = open();
        let preferences = ThemePreferences {
            follow_system: true,
            light: "sage".into(),
            dark: "graphite".into(),
            density: Density::Compact,
            overrides: ThemeOverrides {
                font_size: Some(15),
                radius: Some(8),
                accent: Some("#3b5bdb".into()),
            },
        };
        let appearance = themes(
            f.service
                .handle(Request::SetThemePreferences {
                    preferences: preferences.clone(),
                })
                .await,
        );
        assert_eq!(ThemePreferences::from(&appearance.settings), preferences);
        let theme = f.service.config().theme();
        assert_eq!(theme.density, Density::Compact);
        assert_eq!(theme.tokens.get("color", "accent"), Some("#3b5bdb"));

        let bad = ThemePreferences {
            overrides: ThemeOverrides {
                accent: Some("blue".into()),
                ..ThemeOverrides::default()
            },
            ..preferences
        };
        let Response::Error {
            code, localized, ..
        } = f
            .service
            .handle(Request::SetThemePreferences { preferences: bad })
            .await
        else {
            panic!("expected an error");
        };
        assert_eq!(code, ErrorCode::InvalidRequest);
        assert_eq!(
            localized.unwrap().id,
            "theme-overrides-issue-accent-invalid"
        );
    }

    #[tokio::test]
    async fn follow_windows_switches_with_the_source() {
        let f = open();
        let source = Arc::new(ManualSystemAppearance::new(Some(SystemAppearance::LIGHT)));
        let mut rx = f.service.subscribe();
        let watch = watch_system_appearance(&f.service, Some(source.clone())).unwrap();
        let appearance = f.service.config().appearance();
        assert_eq!(appearance.active, "frost");
        assert_eq!(appearance.system, Some(SystemAppearance::LIGHT));
        drain(&mut rx);

        source.set(SystemAppearance::DARK);
        let events = drain(&mut rx);
        assert!(
            matches!(
                events.as_slice(),
                [Event::ThemeChanged { appearance }, Event::ConfigChanged { .. }] if appearance.active == "graphite"
            ),
            "{events:?}"
        );
        assert_eq!(f.service.config().theme().mode, nail_api::ThemeMode::Dark);

        // A fixed theme ignores the OS (only the OS state in the list changes).
        f.service
            .handle(Request::SetTheme {
                theme_id: "sage".into(),
            })
            .await;
        drain(&mut rx);
        source.set(SystemAppearance::LIGHT);
        let events = drain(&mut rx);
        assert!(
            matches!(events.as_slice(), [Event::ThemeChanged { appearance }] if appearance.active == "sage"),
            "{events:?}"
        );

        // After the guard is dropped, changes are no longer applied.
        drop(watch);
        source.set(SystemAppearance {
            scheme: ColorScheme::Dark,
            high_contrast: true,
        });
        assert!(drain(&mut rx).is_empty());
        assert_eq!(
            f.service.config().system_appearance(),
            Some(SystemAppearance::LIGHT)
        );
    }

    #[tokio::test]
    async fn reported_state_is_applied() {
        let f = open();
        let mut rx = f.service.subscribe();
        let appearance = report_system_appearance(&f.service, Some(SystemAppearance::DARK));
        assert_eq!(appearance.active, "graphite");
        assert_eq!(drain(&mut rx).len(), 2);
        // The same state again: nothing to say.
        report_system_appearance(&f.service, Some(SystemAppearance::DARK));
        assert!(drain(&mut rx).is_empty());
    }

    #[tokio::test]
    async fn other_requests_are_internal_errors() {
        let f = open();
        let err = handle(&f.service, Request::Ping).await.unwrap_err();
        assert_eq!(ApiError::from(err).code, ErrorCode::Internal);
    }

    /// The real Windows source reads the current state and starts and stops
    /// its watcher cleanly (the OS setting itself is never changed here).
    #[cfg(windows)]
    #[test]
    fn the_windows_source_reads_and_watches() {
        let source = WindowsSystemAppearance::new().unwrap();
        let current = source.current();
        assert_eq!(current, windows::read_current());
        if let Ok(key) = windows_registry::CURRENT_USER.open(windows::PERSONALIZE_KEY)
            && let Ok(light) = key.get_u32(windows::APPS_USE_LIGHT_THEME)
        {
            let scheme = if light == 0 {
                ColorScheme::Dark
            } else {
                ColorScheme::Light
            };
            assert_eq!(current.map(|c| c.scheme), Some(scheme));
        }
        let handle = source.watch(Arc::new(|_| {}));
        let second = source.watch(Arc::new(|_| {}));
        assert!(format!("{source:?}").contains("listeners: 2"));
        source.unwatch(handle);
        source.unwatch(second);
        assert!(format!("{source:?}").contains("listeners: 0"));
        let started = std::time::Instant::now();
        drop(source);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "the watcher thread stops at once"
        );
        assert!(platform_source().is_some());
    }
}
