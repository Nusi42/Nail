//! Pop-out windows (M5c WP173, ADRs 0116 and 0119): the shell's
//! side of `PopOutWindow` / `ReturnPopOut` / `ClosePopOut` / `FocusPopOut`.
//!
//! The core keeps the list of open pop-outs and asks the shell, through
//! [`nail_api::DesktopHost`], to open, close and focus OS windows. Each
//! pop-out is a Tauri `WebviewWindow` labelled `popout-<id>` that loads the
//! same UI at `#/window/<id>` and talks to the same in-process core through
//! the same `nail_call` command; core events reach every webview of the
//! app, so each pop-out keeps itself current like the main window.
//!
//! [`PopOutManager`] holds what does not need Tauri — placing a window on a
//! screen that still exists ([`place`]), tracking the restored rectangle
//! of a maximized window, reporting moves when they end (or at most every
//! [`REPORT_MAX_DELAY`]), reporting focus and closes as
//! [`DesktopChange::PopOut`], and closing every pop-out when the main
//! window closes. The OS windows sit behind [`PopOutWindows`]
//! ([`TauriPopOuts`] in the app, a fake in the tests).
//!
//! Geometry is in physical screen pixels (what Windows positions windows
//! by), so a pop-out reopens exactly where it was on any monitor whatever
//! its scale factor.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use nail_api::desktop::{DesktopChange, DesktopNotifier};
use nail_api::windows::{
    DEFAULT_WINDOW_SPEC, MAX_POP_OUT_TITLE_CHARS, POP_OUT_LABEL_PREFIX, PopOutChange, PopOutIssue,
    PopOutOpen, PopOutReport, WINDOW_CASCADE_OFFSET, WINDOW_MIN_VISIBLE, WindowGeometry,
    WindowRect, WindowSize, parse_pop_out_label, pop_out_label,
};
use nail_api::{ApiError, ErrorCode, LocalizedMessage};
use tokio::time::Instant;

/// A move / resize is reported once the window kept still this long.
pub const REPORT_QUIET: Duration = Duration::from_millis(300);
/// During a long move / resize, a report goes out at least this often.
pub const REPORT_MAX_DELAY: Duration = Duration::from_millis(500);
/// The height of the strip at a window's top that must stay on a screen so
/// the user can grab the title bar (CSS pixels).
pub const TITLE_STRIP: u32 = 32;
/// The smallest pop-out (CSS pixels).
pub const MIN_POP_OUT_SIZE: WindowSize = WindowSize::new(320, 240);
/// How often a new window steps aside from one at the same place.
const MAX_CASCADE_STEPS: usize = 32;

/// What the shell knows about the screens when it places a pop-out.
#[derive(Debug, Clone, PartialEq)]
pub struct Screen {
    /// The monitors' work areas (without the taskbar), the primary first.
    pub work_areas: Vec<WindowRect>,
    /// The main window's rectangle when it is shown (a new pop-out opens
    /// over it).
    pub anchor: Option<WindowRect>,
    /// The main window's scale factor (CSS → physical pixels).
    pub scale: f64,
}

impl Screen {
    fn scale(&self) -> f64 {
        if self.scale.is_finite() {
            self.scale.clamp(0.5, 8.0)
        } else {
            1.0
        }
    }

    fn px(&self, css: u32) -> u32 {
        (f64::from(css) * self.scale()).round() as u32
    }
}

fn right(r: &WindowRect) -> i64 {
    i64::from(r.x) + i64::from(r.width)
}

fn bottom(r: &WindowRect) -> i64 {
    i64::from(r.y) + i64::from(r.height)
}

fn contains(area: &WindowRect, x: i64, y: i64) -> bool {
    x >= i64::from(area.x) && x < right(area) && y >= i64::from(area.y) && y < bottom(area)
}

fn to_i32(v: i64) -> i32 {
    v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// Whether the user can grab `rect`'s title bar on `area`: its top strip
/// ([`TITLE_STRIP`], `strip` physical pixels) lies inside the area
/// vertically and at least `min_visible` pixels of it horizontally.
pub fn grabbable_on(rect: &WindowRect, area: &WindowRect, strip: u32, min_visible: u32) -> bool {
    let strip = i64::from(strip.min(rect.height));
    let top = i64::from(rect.y);
    let overlap = right(rect).min(right(area)) - i64::from(rect.x).max(i64::from(area.x));
    top >= i64::from(area.y)
        && top + strip <= bottom(area)
        && overlap >= i64::from(min_visible.min(rect.width))
}

/// Where a pop-out opens: the remembered geometry when its title bar is
/// still on one of the screens (shrunk to fit that screen), else the
/// remembered (or default) size centred over the main window on its
/// screen — a monitor that was unplugged never swallows a window. A window
/// at the same place as an open one (`taken`) steps aside by
/// [`WINDOW_CASCADE_OFFSET`].
pub fn place(
    requested: Option<&WindowGeometry>,
    screen: &Screen,
    taken: &[WindowRect],
) -> WindowGeometry {
    let strip = screen.px(TITLE_STRIP);
    let min_visible = screen.px(WINDOW_MIN_VISIBLE);
    let default_size = (
        screen.px(DEFAULT_WINDOW_SPEC.size.width),
        screen.px(DEFAULT_WINDOW_SPEC.size.height),
    );
    let fit = |area: &WindowRect, (w, h): (u32, u32)| (w.min(area.width), h.min(area.height));

    let kept = requested.and_then(|g| {
        screen
            .work_areas
            .iter()
            .find(|area| grabbable_on(&g.rect, area, strip, min_visible))
            .map(|area| (*g, *area))
    });
    let (mut geometry, area) = match kept {
        Some((g, area)) => {
            let (width, height) = fit(&area, (g.rect.width, g.rect.height));
            let rect = WindowRect {
                width,
                height,
                ..g.rect
            };
            (
                WindowGeometry {
                    rect,
                    maximized: g.maximized,
                    snap: None,
                },
                Some(area),
            )
        }
        None => {
            let size = requested.map_or(default_size, |g| (g.rect.width, g.rect.height));
            let centre = screen.anchor.map(|a| {
                (
                    i64::from(a.x) + i64::from(a.width) / 2,
                    i64::from(a.y) + i64::from(a.height) / 2,
                )
            });
            let area = centre
                .and_then(|(x, y)| {
                    screen
                        .work_areas
                        .iter()
                        .find(|area| contains(area, x, y))
                        .copied()
                })
                .or_else(|| screen.work_areas.first().copied());
            let rect = match area {
                Some(area) => {
                    let (width, height) = fit(&area, size);
                    let (cx, cy) = centre.filter(|&(x, y)| contains(&area, x, y)).unwrap_or((
                        i64::from(area.x) + i64::from(area.width) / 2,
                        i64::from(area.y) + i64::from(area.height) / 2,
                    ));
                    let x = (cx - i64::from(width) / 2)
                        .clamp(i64::from(area.x), right(&area) - i64::from(width));
                    let y = (cy - i64::from(height) / 2)
                        .clamp(i64::from(area.y), bottom(&area) - i64::from(height));
                    WindowRect {
                        x: to_i32(x),
                        y: to_i32(y),
                        width,
                        height,
                    }
                }
                // No monitor known: the main window's centre, else a corner.
                None => {
                    let (width, height) = size;
                    let (x, y) = centre.map_or(
                        (i64::from(min_visible), i64::from(min_visible)),
                        |(x, y)| (x - i64::from(width) / 2, y - i64::from(height) / 2),
                    );
                    WindowRect {
                        x: to_i32(x),
                        y: to_i32(y),
                        width,
                        height,
                    }
                }
            };
            (WindowGeometry::at(rect), area)
        }
    };

    let offset = i64::from(screen.px(WINDOW_CASCADE_OFFSET));
    let mut steps = 0;
    while steps < MAX_CASCADE_STEPS
        && taken
            .iter()
            .any(|t| t.x == geometry.rect.x && t.y == geometry.rect.y)
    {
        let rect = &mut geometry.rect;
        let (mut x, mut y) = (i64::from(rect.x) + offset, i64::from(rect.y) + offset);
        if let Some(area) = area
            && (x + i64::from(rect.width) > right(&area)
                || y + i64::from(rect.height) > bottom(&area))
        {
            // Off the screen's edge: start again at its top left corner.
            x = i64::from(area.x) + offset * i64::try_from(steps).unwrap_or(0);
            y = i64::from(area.y);
        }
        rect.x = to_i32(x);
        rect.y = to_i32(y);
        steps += 1;
    }
    geometry
}

/// The OS window title for a pop-out's document title (the UI sets it to
/// the window's title, e.g. a composer's subject): trimmed, without control
/// characters, at most [`MAX_POP_OUT_TITLE_CHARS`]; `None` when empty.
pub fn taskbar_title(document_title: &str) -> Option<String> {
    let clean: String = document_title
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let trimmed = clean.trim();
    (!trimmed.is_empty()).then(|| trimmed.chars().take(MAX_POP_OUT_TITLE_CHARS).collect())
}

/// The URL a pop-out's webview loads (the app's own files).
pub fn pop_out_url(route: &str) -> String {
    format!("index.html{route}")
}

/// When a move / resize is reported: once the window kept still for
/// [`REPORT_QUIET`], or [`REPORT_MAX_DELAY`] after the first unreported
/// change.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReportClock {
    first: Option<Instant>,
    last: Option<Instant>,
}

impl ReportClock {
    /// The window moved at `now`; `true` when no report was pending (start
    /// waiting for [`Self::deadline`]).
    pub fn note(&mut self, now: Instant) -> bool {
        let start = self.first.is_none();
        self.first.get_or_insert(now);
        self.last = Some(now);
        start
    }

    /// When the pending report is due, `None` when none is pending.
    pub fn deadline(&self) -> Option<Instant> {
        Some((self.last? + REPORT_QUIET).min(self.first? + REPORT_MAX_DELAY))
    }

    /// Whether the pending report is due at `now` (it is then taken).
    pub fn take_due(&mut self, now: Instant) -> bool {
        match self.deadline() {
            Some(due) if now >= due => {
                *self = Self::default();
                true
            }
            _ => false,
        }
    }
}

/// A pop-out's position and state as the OS shows it (physical pixels:
/// outer position, inner size).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Observed {
    /// Where it is.
    pub rect: WindowRect,
    /// Maximized.
    pub maximized: bool,
    /// Minimized (not reported: the restored geometry stays).
    pub minimized: bool,
}

/// What a pop-out's OS window tells the manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// Moved, resized, maximized or restored, or its scale factor changed.
    Moved,
    /// It got the focus.
    Focused,
    /// It was closed (the user's close button, or the shell destroyed it).
    Destroyed,
}

/// Where an OS window's signals go.
pub type PopOutSink = Arc<dyn Fn(Signal) + Send + Sync>;

/// What [`PopOutWindows::open`] builds.
pub struct OpenSpec {
    /// The OS window label (`popout-<id>`).
    pub label: String,
    /// What the webview loads ([`pop_out_url`]).
    pub url: String,
    /// The first title (the UI's document title replaces it,
    /// [`taskbar_title`]).
    pub title: String,
    /// Where (physical pixels: outer position, inner size), maximized or
    /// not.
    pub geometry: WindowGeometry,
    /// The smallest inner size (CSS pixels).
    pub min_size: WindowSize,
    /// Where its moves, focus and close go.
    pub sink: PopOutSink,
}

impl std::fmt::Debug for OpenSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenSpec")
            .field("label", &self.label)
            .field("url", &self.url)
            .field("title", &self.title)
            .field("geometry", &self.geometry)
            .finish_non_exhaustive()
    }
}

/// The OS windows of pop-outs ([`TauriPopOuts`]; a fake in tests). Calls
/// come from the core's tasks, never from the UI thread.
pub trait PopOutWindows: Send + Sync {
    /// The screens and the main window.
    fn screen(&self) -> Screen;
    /// Whether a window with this label exists.
    fn exists(&self, label: &str) -> bool;
    /// Builds and shows the window, focused.
    fn open(&self, spec: OpenSpec) -> Result<(), String>;
    /// Destroys the window; `false` when there was none.
    fn close(&self, label: &str) -> Result<bool, String>;
    /// Shows, restores and focuses the window; `false` when there is none.
    fn focus(&self, label: &str) -> Result<bool, String>;
    /// Where the window is now (`None` = gone or unknown).
    fn observe(&self, label: &str) -> Option<Observed>;
}

#[derive(Debug, Default)]
struct Tracked {
    /// The last rectangle while not maximized (restoring brings it back).
    normal: Option<WindowRect>,
    /// The last rectangle seen (cascading new windows).
    last: Option<WindowRect>,
    clock: ReportClock,
}

impl Tracked {
    fn at(geometry: &WindowGeometry) -> Self {
        Self {
            normal: Some(geometry.rect),
            last: Some(geometry.rect),
            clock: ReportClock::default(),
        }
    }

    /// The geometry to report for what the OS shows (`None` while
    /// minimized or not storable).
    fn geometry(&mut self, seen: Observed) -> Option<WindowGeometry> {
        if seen.minimized {
            return None;
        }
        if !seen.maximized {
            self.normal = Some(seen.rect);
        }
        self.last = Some(seen.rect);
        let geometry = WindowGeometry {
            rect: self.normal.unwrap_or(seen.rect),
            maximized: seen.maximized,
            snap: None,
        };
        geometry.is_valid().then_some(geometry)
    }
}

/// The shell's pop-out windows: opens, closes and focuses them and reports
/// what the user does with them to the core.
pub struct PopOutManager {
    windows: OnceLock<Arc<dyn PopOutWindows>>,
    notify: OnceLock<DesktopNotifier>,
    tracked: Mutex<HashMap<String, Tracked>>,
    runtime: tokio::runtime::Handle,
    me: Weak<PopOutManager>,
}

impl std::fmt::Debug for PopOutManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PopOutManager")
            .field("open", &self.open_windows())
            .finish_non_exhaustive()
    }
}

fn open_error(err: impl std::fmt::Display) -> ApiError {
    ApiError::internal(format!("cannot open the pop-out window: {err}"))
        .with_localized(LocalizedMessage::new("popout-open-failed"))
}

impl PopOutManager {
    /// A manager whose timers run on `runtime`.
    pub fn new(runtime: tokio::runtime::Handle) -> Arc<Self> {
        Arc::new_cyclic(|me| Self {
            windows: OnceLock::new(),
            notify: OnceLock::new(),
            tracked: Mutex::new(HashMap::new()),
            runtime,
            me: me.clone(),
        })
    }

    /// The OS windows, once the app runs.
    pub fn bind(&self, windows: Arc<dyn PopOutWindows>) {
        let _ = self.windows.set(windows);
    }

    /// The core's notifier.
    pub fn attach(&self, notify: DesktopNotifier) {
        let _ = self.notify.set(notify);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Tracked>> {
        self.tracked
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn backend(&self) -> Result<Arc<dyn PopOutWindows>, ApiError> {
        self.windows
            .get()
            .cloned()
            .ok_or_else(|| ApiError::new(ErrorCode::Unavailable, "the app is not running yet"))
    }

    /// The open pop-outs' window ids, sorted.
    pub fn open_windows(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.lock().keys().cloned().collect();
        ids.sort();
        ids
    }

    fn report(&self, window: &str, change: PopOutChange, geometry: Option<WindowGeometry>) {
        if let Some(notify) = self.notify.get() {
            notify(DesktopChange::PopOut(PopOutReport {
                window: window.to_owned(),
                change,
                geometry,
            }));
        }
    }

    fn sink(&self, window: &str) -> PopOutSink {
        let me = self.me.clone();
        let window = window.to_owned();
        Arc::new(move |signal| {
            if let Some(manager) = me.upgrade() {
                manager.signal(&window, signal);
            }
        })
    }

    /// Opens `open.window` as an OS window titled `title`, placed on a
    /// screen ([`place`]); an existing one is focused instead.
    pub fn open(&self, open: &PopOutOpen, title: String) -> Result<(), ApiError> {
        let windows = self.backend()?;
        let label = pop_out_label(&open.window);
        if windows.exists(&label) {
            windows.focus(&label).map_err(open_error)?;
            return Ok(());
        }
        let screen = windows.screen();
        let taken: Vec<WindowRect> = self.lock().values().filter_map(|t| t.last).collect();
        let geometry = place(open.geometry.as_ref(), &screen, &taken);
        // Tracked before the window exists: it reports moves while it is
        // being placed.
        self.lock()
            .insert(open.window.clone(), Tracked::at(&geometry));
        let spec = OpenSpec {
            label,
            url: pop_out_url(&open.route),
            title,
            geometry,
            min_size: MIN_POP_OUT_SIZE,
            sink: self.sink(&open.window),
        };
        if let Err(err) = windows.open(spec) {
            self.lock().remove(&open.window);
            return Err(open_error(err));
        }
        // Remember where it really opened (the core lists it by then).
        self.schedule(&open.window);
        Ok(())
    }

    /// Closes a pop-out the core removed (`ReturnPopOut` / `ClosePopOut`);
    /// a window that is already gone is fine.
    pub fn close(&self, window: &str) -> Result<(), ApiError> {
        let windows = self.backend()?;
        self.lock().remove(window);
        windows
            .close(&pop_out_label(window))
            .map(|_| ())
            .map_err(|e| ApiError::internal(format!("cannot close the pop-out window: {e}")))
    }

    /// Brings a pop-out to the front; one that is gone is reported closed
    /// (`conflict`).
    pub fn focus(&self, window: &str) -> Result<(), ApiError> {
        let windows = self.backend()?;
        let found = windows
            .focus(&pop_out_label(window))
            .map_err(|e| ApiError::internal(format!("cannot focus the pop-out window: {e}")))?;
        if found {
            return Ok(());
        }
        if self.lock().remove(window).is_some() {
            self.report(window, PopOutChange::Closed, None);
        }
        Err(
            ApiError::new(ErrorCode::Conflict, format!("pop-out `{window}` is gone"))
                .with_localized(LocalizedMessage::new(PopOutIssue::NotOpen.message_id())),
        )
    }

    /// The main window closes: every pop-out closes with it (reported as
    /// closed, so its kind opens as an OS window again next time).
    pub fn close_all(&self) {
        let open: Vec<String> = self.lock().drain().map(|(window, _)| window).collect();
        let windows = self.windows.get().cloned();
        for window in open {
            if let Some(windows) = &windows
                && let Err(err) = windows.close(&pop_out_label(&window))
            {
                tracing::warn!(%err, window, "cannot close a pop-out window");
            }
            self.report(&window, PopOutChange::Closed, None);
        }
    }

    /// A signal of a pop-out's OS window.
    pub fn signal(&self, window: &str, signal: Signal) {
        match signal {
            Signal::Moved => self.schedule(window),
            Signal::Focused => {
                if self.lock().contains_key(window) {
                    self.report(window, PopOutChange::Focused, None);
                }
            }
            Signal::Destroyed => {
                // The user closed it (a close the core asked for was
                // forgotten already).
                if self.lock().remove(window).is_some() {
                    self.report(window, PopOutChange::Closed, None);
                }
            }
        }
    }

    /// Notes a move and makes sure a report follows ([`ReportClock`]).
    fn schedule(&self, window: &str) {
        let start = match self.lock().get_mut(window) {
            Some(tracked) => tracked.clock.note(Instant::now()),
            None => return,
        };
        if !start {
            return;
        }
        let me = self.me.clone();
        let window = window.to_owned();
        self.runtime.spawn(async move {
            loop {
                let deadline = {
                    let Some(manager) = me.upgrade() else { return };
                    let tracked = manager.lock();
                    let Some(deadline) = tracked.get(&window).and_then(|t| t.clock.deadline())
                    else {
                        return;
                    };
                    deadline
                };
                tokio::time::sleep_until(deadline).await;
                let Some(manager) = me.upgrade() else { return };
                let due = match manager.lock().get_mut(&window) {
                    Some(tracked) => tracked.clock.take_due(Instant::now()),
                    None => return,
                };
                if due {
                    manager.report_geometry(&window);
                    return;
                }
            }
        });
    }

    fn report_geometry(&self, window: &str) {
        let Some(windows) = self.windows.get() else {
            return;
        };
        let Some(seen) = windows.observe(&pop_out_label(window)) else {
            return;
        };
        let geometry = match self.lock().get_mut(window) {
            Some(tracked) => tracked.geometry(seen),
            None => return,
        };
        if let Some(geometry) = geometry {
            self.report(window, PopOutChange::Moved, Some(geometry));
        }
    }
}

/// Whether an OS window label is a pop-out's.
pub fn is_pop_out_label(label: &str) -> bool {
    label.starts_with(POP_OUT_LABEL_PREFIX) && parse_pop_out_label(label).is_some()
}

// ---------------------------------------------------------------------------
// Tauri

/// The pop-outs as Tauri webview windows sharing the main window's
/// WebView2 data folder.
pub struct TauriPopOuts<R: tauri::Runtime> {
    app: tauri::AppHandle<R>,
    webview_dir: Option<std::path::PathBuf>,
}

impl<R: tauri::Runtime> TauriPopOuts<R> {
    /// Pop-outs of `app` (`webview_dir` = the main window's data folder).
    pub fn new(app: tauri::AppHandle<R>, webview_dir: Option<std::path::PathBuf>) -> Self {
        Self { app, webview_dir }
    }

    fn window(&self, label: &str) -> Option<tauri::WebviewWindow<R>> {
        use tauri::Manager as _;
        self.app.get_webview_window(label)
    }
}

fn rect_of(position: tauri::PhysicalPosition<i32>, size: tauri::PhysicalSize<u32>) -> WindowRect {
    WindowRect {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    }
}

impl<R: tauri::Runtime> PopOutWindows for TauriPopOuts<R> {
    fn screen(&self) -> Screen {
        let mut monitors = self.app.available_monitors().unwrap_or_default();
        if let Ok(Some(primary)) = self.app.primary_monitor()
            && let Some(at) = monitors
                .iter()
                .position(|m| m.position() == primary.position())
        {
            let primary = monitors.remove(at);
            monitors.insert(0, primary);
        }
        let work_areas = monitors
            .iter()
            .map(|m| rect_of(m.work_area().position, m.work_area().size))
            .collect();
        let main = self.window(crate::shell::MAIN_WINDOW);
        let anchor = main
            .as_ref()
            .filter(|w| w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false))
            .and_then(|w| Some(rect_of(w.outer_position().ok()?, w.outer_size().ok()?)));
        let scale = main
            .as_ref()
            .and_then(|w| w.scale_factor().ok())
            .unwrap_or(1.0);
        Screen {
            work_areas,
            anchor,
            scale,
        }
    }

    fn exists(&self, label: &str) -> bool {
        self.window(label).is_some()
    }

    fn open(&self, spec: OpenSpec) -> Result<(), String> {
        let url = tauri::WebviewUrl::App(spec.url.into());
        let mut builder = tauri::WebviewWindowBuilder::new(&self.app, &spec.label, url)
            .title(&spec.title)
            .visible(false)
            .min_inner_size(
                f64::from(spec.min_size.width),
                f64::from(spec.min_size.height),
            )
            .on_document_title_changed(|window, title| {
                if let Some(title) = taskbar_title(&title)
                    && let Err(err) = window.set_title(&title)
                {
                    tracing::warn!(%err, "cannot retitle a pop-out window");
                }
            });
        if let Some(dir) = &self.webview_dir {
            builder = builder.data_directory(dir.clone());
        }
        let window = builder.build().map_err(|e| e.to_string())?;
        let sink = Arc::clone(&spec.sink);
        window.on_window_event(move |event| match event {
            tauri::WindowEvent::Moved(_)
            | tauri::WindowEvent::Resized(_)
            | tauri::WindowEvent::ScaleFactorChanged { .. } => sink(Signal::Moved),
            tauri::WindowEvent::Focused(true) => sink(Signal::Focused),
            tauri::WindowEvent::Destroyed => sink(Signal::Destroyed),
            _ => {}
        });
        let rect = spec.geometry.rect;
        let placed = window
            .set_position(tauri::PhysicalPosition::new(rect.x, rect.y))
            .and_then(|()| window.set_size(tauri::PhysicalSize::new(rect.width, rect.height)))
            .and_then(|()| {
                if spec.geometry.maximized {
                    window.maximize()
                } else {
                    Ok(())
                }
            })
            .and_then(|()| window.show())
            .and_then(|()| window.set_focus());
        if let Err(err) = placed {
            let _ = window.destroy();
            return Err(err.to_string());
        }
        Ok(())
    }

    fn close(&self, label: &str) -> Result<bool, String> {
        match self.window(label) {
            Some(window) => window.destroy().map(|()| true).map_err(|e| e.to_string()),
            None => Ok(false),
        }
    }

    fn focus(&self, label: &str) -> Result<bool, String> {
        match self.window(label) {
            Some(window) => window
                .show()
                .and_then(|()| window.unminimize())
                .and_then(|()| window.set_focus())
                .map(|()| true)
                .map_err(|e| e.to_string()),
            None => Ok(false),
        }
    }

    fn observe(&self, label: &str) -> Option<Observed> {
        let window = self.window(label)?;
        Some(Observed {
            rect: rect_of(window.outer_position().ok()?, window.inner_size().ok()?),
            maximized: window.is_maximized().unwrap_or(false),
            minimized: window.is_minimized().unwrap_or(false),
        })
    }
}

#[cfg(test)]
pub(crate) mod testing {
    //! A fake [`PopOutWindows`] for tests (also used by `host.rs`).

    use super::*;

    /// One fake OS window.
    pub struct FakeWindow {
        pub spec: OpenSpec,
        pub seen: Observed,
        pub focus_count: usize,
    }

    /// Records what the manager asks and lets tests play the user.
    #[derive(Default)]
    pub struct FakeWindows {
        pub screen: Mutex<Option<Screen>>,
        pub windows: Mutex<HashMap<String, FakeWindow>>,
        pub closed: Mutex<Vec<String>>,
        pub fail_open: std::sync::atomic::AtomicBool,
    }

    impl FakeWindows {
        /// One 1920 × 1040 work area (a 40 px taskbar), the main window
        /// at 100,100 1280 × 800, scale 1.
        pub fn single_monitor() -> Arc<Self> {
            let fake = Self::default();
            *fake.screen.lock().unwrap() = Some(Screen {
                work_areas: vec![WindowRect {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1040,
                }],
                anchor: Some(WindowRect {
                    x: 100,
                    y: 100,
                    width: 1280,
                    height: 800,
                }),
                scale: 1.0,
            });
            Arc::new(fake)
        }

        /// The user moved / resized / maximized a window (the OS fires
        /// its event).
        pub fn user_moves(&self, label: &str, rect: WindowRect, maximized: bool) {
            let sink = {
                let mut windows = self.windows.lock().unwrap();
                let window = windows.get_mut(label).expect("open");
                window.seen = Observed {
                    rect,
                    maximized,
                    minimized: false,
                };
                Arc::clone(&window.spec.sink)
            };
            sink(Signal::Moved);
        }

        /// The user clicked a window.
        pub fn user_focuses(&self, label: &str) {
            let sink = Arc::clone(&self.windows.lock().unwrap()[label].spec.sink);
            sink(Signal::Focused);
        }

        /// The user clicked a window's close button.
        pub fn user_closes(&self, label: &str) {
            let window = self.windows.lock().unwrap().remove(label).expect("open");
            (window.spec.sink)(Signal::Destroyed);
        }

        pub fn labels(&self) -> Vec<String> {
            let mut labels: Vec<String> = self.windows.lock().unwrap().keys().cloned().collect();
            labels.sort();
            labels
        }

        pub fn geometry_of(&self, label: &str) -> WindowGeometry {
            self.windows.lock().unwrap()[label].spec.geometry
        }

        pub fn title_of(&self, label: &str) -> String {
            self.windows.lock().unwrap()[label].spec.title.clone()
        }
    }

    impl PopOutWindows for FakeWindows {
        fn screen(&self) -> Screen {
            self.screen.lock().unwrap().clone().unwrap_or(Screen {
                work_areas: vec![],
                anchor: None,
                scale: 1.0,
            })
        }

        fn exists(&self, label: &str) -> bool {
            self.windows.lock().unwrap().contains_key(label)
        }

        fn open(&self, spec: OpenSpec) -> Result<(), String> {
            if self.fail_open.load(std::sync::atomic::Ordering::Relaxed) {
                return Err("no more windows".into());
            }
            let seen = Observed {
                rect: spec.geometry.rect,
                maximized: spec.geometry.maximized,
                minimized: false,
            };
            let label = spec.label.clone();
            let sink = Arc::clone(&spec.sink);
            self.windows.lock().unwrap().insert(
                label,
                FakeWindow {
                    spec,
                    seen,
                    focus_count: 1,
                },
            );
            // Placing a window moves it.
            sink(Signal::Moved);
            Ok(())
        }

        fn close(&self, label: &str) -> Result<bool, String> {
            // Destroying a window fires `Destroyed` like the real one.
            let window = self.windows.lock().unwrap().remove(label);
            self.closed.lock().unwrap().push(label.to_owned());
            match window {
                Some(window) => {
                    (window.spec.sink)(Signal::Destroyed);
                    Ok(true)
                }
                None => Ok(false),
            }
        }

        fn focus(&self, label: &str) -> Result<bool, String> {
            Ok(self
                .windows
                .lock()
                .unwrap()
                .get_mut(label)
                .map(|w| w.focus_count += 1)
                .is_some())
        }

        fn observe(&self, label: &str) -> Option<Observed> {
            self.windows.lock().unwrap().get(label).map(|w| w.seen)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::FakeWindows;
    use super::*;
    use nail_api::windows::{WindowSnap, pop_out_route};
    use pretty_assertions::assert_eq;

    fn rect(x: i32, y: i32, width: u32, height: u32) -> WindowRect {
        WindowRect {
            x,
            y,
            width,
            height,
        }
    }

    /// Two monitors: the primary 1920 × 1040 at the origin, a 2560 × 1400
    /// one to its left at 150 % (physical pixels).
    fn two_monitors(anchor: Option<WindowRect>) -> Screen {
        Screen {
            work_areas: vec![rect(0, 0, 1920, 1040), rect(-2560, -200, 2560, 1400)],
            anchor,
            scale: 1.0,
        }
    }

    #[test]
    fn a_remembered_place_on_a_screen_is_kept() {
        let screen = two_monitors(Some(rect(100, 100, 1280, 800)));
        let on_left = WindowGeometry::at(rect(-2000, 0, 900, 700));
        assert_eq!(place(Some(&on_left), &screen, &[]), on_left);
        // Maximized stays maximized (with its restored rectangle).
        let maximized = WindowGeometry {
            maximized: true,
            ..WindowGeometry::at(rect(300, 200, 800, 600))
        };
        assert_eq!(place(Some(&maximized), &screen, &[]), maximized);
        // A snapped window is not snapped by the shell (Windows snaps).
        let snapped = WindowGeometry {
            snap: Some(WindowSnap::Left),
            ..WindowGeometry::at(rect(0, 0, 960, 1040))
        };
        assert_eq!(place(Some(&snapped), &screen, &[]).snap, None);
    }

    #[test]
    fn a_window_on_an_unplugged_monitor_comes_back_over_the_main_window() {
        let screen = Screen {
            work_areas: vec![rect(0, 0, 1920, 1040)],
            anchor: Some(rect(100, 100, 1280, 800)),
            scale: 1.0,
        };
        // It was on the left monitor, which is gone: same size, centred
        // over the main window (centre 740,500).
        let gone = WindowGeometry::at(rect(-2000, 0, 900, 700));
        assert_eq!(
            place(Some(&gone), &screen, &[]),
            WindowGeometry::at(rect(290, 150, 900, 700))
        );
        // A window larger than the screen shrinks to it.
        let huge = WindowGeometry::at(rect(-5000, 0, 4000, 3000));
        assert_eq!(
            place(Some(&huge), &screen, &[]).rect,
            rect(0, 0, 1920, 1040)
        );
        // Title bar above the screen's top edge: not grabbable, moved.
        let above = WindowGeometry::at(rect(200, -20, 600, 400));
        assert_eq!(place(Some(&above), &screen, &[]).rect.y, 300);
        // Only a sliver on screen: moved too.
        let sliver = WindowGeometry::at(rect(1900, 100, 600, 400));
        assert_eq!(place(Some(&sliver), &screen, &[]).rect.x, 440);
        // 48 px of the title bar are enough.
        let enough = WindowGeometry::at(rect(1872, 100, 600, 400));
        assert_eq!(place(Some(&enough), &screen, &[]), enough);
    }

    #[test]
    fn a_new_window_opens_centred_at_the_default_size_scaled() {
        // The main window on the left monitor: the pop-out opens there.
        let screen = Screen {
            scale: 1.5,
            ..two_monitors(Some(rect(-2400, 0, 1600, 1000)))
        };
        let placed = place(None, &screen, &[]);
        // 720 × 520 CSS px at 150 %, centred on the main window (-1600,500).
        assert_eq!(placed, WindowGeometry::at(rect(-2140, 110, 1080, 780)));
        // Hidden main window: the primary screen's centre.
        let placed = place(None, &two_monitors(None), &[]);
        assert_eq!(placed.rect, rect(600, 260, 720, 520));
        // A main window hanging off the screen: the pop-out stays on it.
        let screen = two_monitors(Some(rect(1500, 700, 1280, 800)));
        let placed = place(None, &screen, &[]);
        assert!(placed.rect.x as i64 + placed.rect.width as i64 <= 1920);
        assert!(placed.rect.y as i64 + placed.rect.height as i64 <= 1040);
        // No monitor known (should not happen): over the main window.
        let screen = Screen {
            work_areas: vec![],
            anchor: Some(rect(0, 0, 1000, 800)),
            scale: 1.0,
        };
        assert_eq!(place(None, &screen, &[]).rect, rect(140, 140, 720, 520));
    }

    #[test]
    fn a_second_window_at_the_same_place_cascades() {
        let screen = two_monitors(Some(rect(100, 100, 1280, 800)));
        let first = place(None, &screen, &[]);
        let second = place(None, &screen, &[first.rect]);
        assert_eq!(
            (second.rect.x, second.rect.y),
            (first.rect.x + 24, first.rect.y + 24)
        );
        let third = place(None, &screen, &[first.rect, second.rect]);
        assert_eq!(third.rect.x, first.rect.x + 48);
        // At the bottom right edge it starts again at the top left.
        let corner = WindowGeometry::at(rect(1200, 520, 720, 520));
        let next = place(Some(&corner), &screen, &[corner.rect]);
        assert_eq!((next.rect.x, next.rect.y), (0, 0));
        // Never loops forever.
        let all: Vec<WindowRect> = (0..200).map(|i| rect(i * 24, i * 24, 1, 1)).collect();
        let _ = place(
            Some(&WindowGeometry::at(rect(0, 0, 400, 300))),
            &screen,
            &all,
        );
    }

    #[test]
    fn taskbar_titles_are_cleaned() {
        assert_eq!(taskbar_title("  Re: Lunch  ").as_deref(), Some("Re: Lunch"));
        assert_eq!(taskbar_title("a\nb\u{7}").as_deref(), Some("a b"));
        assert_eq!(taskbar_title(" \t "), None);
        assert_eq!(
            taskbar_title(&"x".repeat(1000)).map(|t| t.chars().count()),
            Some(MAX_POP_OUT_TITLE_CHARS)
        );
    }

    #[test]
    fn urls_and_labels() {
        assert_eq!(
            pop_out_url(&pop_out_route("composer:d42")),
            "index.html#/window/composer:d42"
        );
        assert!(is_pop_out_label("popout-composer:d42"));
        assert!(is_pop_out_label("popout-settings"));
        assert!(!is_pop_out_label("main"));
        assert!(!is_pop_out_label("print-1"));
        assert!(!is_pop_out_label("popout-Bad Kind"));
    }

    #[test]
    fn the_report_clock_waits_for_quiet_or_the_maximum_delay() {
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        let mut clock = ReportClock::default();
        assert_eq!(clock.deadline(), None);
        assert!(!clock.take_due(t0));
        assert!(clock.note(t0), "the first move starts the wait");
        assert!(!clock.note(t0 + ms(100)), "later moves join it");
        assert_eq!(clock.deadline(), Some(t0 + ms(400)));
        assert!(!clock.take_due(t0 + ms(399)));
        // Still moving: at most 500 ms after the first move.
        clock.note(t0 + ms(350));
        assert_eq!(clock.deadline(), Some(t0 + ms(500)));
        assert!(clock.take_due(t0 + ms(500)));
        assert_eq!(clock.deadline(), None, "taken");
        assert!(clock.note(t0 + ms(600)));
    }

    #[test]
    fn a_maximized_window_keeps_its_restored_rectangle() {
        let mut tracked = Tracked::at(&WindowGeometry::at(rect(10, 10, 800, 600)));
        let seen = |rect, maximized, minimized| Observed {
            rect,
            maximized,
            minimized,
        };
        assert_eq!(
            tracked.geometry(seen(rect(50, 60, 800, 600), false, false)),
            Some(WindowGeometry::at(rect(50, 60, 800, 600)))
        );
        // Maximized: the OS reports the screen; the restored one stays.
        assert_eq!(
            tracked.geometry(seen(rect(-8, -8, 1936, 1056), true, false)),
            Some(WindowGeometry {
                maximized: true,
                ..WindowGeometry::at(rect(50, 60, 800, 600))
            })
        );
        // Minimized: nothing to report.
        assert_eq!(
            tracked.geometry(seen(rect(-32000, -32000, 160, 28), false, true)),
            None
        );
        // Not storable (an empty window): nothing either.
        assert_eq!(tracked.geometry(seen(rect(0, 0, 0, 0), false, false)), None);
    }

    fn open_request(window: &str, geometry: Option<WindowGeometry>) -> PopOutOpen {
        PopOutOpen {
            window: window.into(),
            label: pop_out_label(window),
            route: pop_out_route(window),
            title: None,
            geometry,
        }
    }

    type Reports = Arc<Mutex<Vec<PopOutReport>>>;

    fn manager() -> (Arc<PopOutManager>, Arc<FakeWindows>, Reports) {
        let manager = PopOutManager::new(tokio::runtime::Handle::current());
        let fake = FakeWindows::single_monitor();
        manager.bind(Arc::clone(&fake) as Arc<dyn PopOutWindows>);
        let reports: Reports = Arc::default();
        let sink = Arc::clone(&reports);
        manager.attach(Arc::new(move |change| {
            if let DesktopChange::PopOut(report) = change {
                sink.lock().unwrap().push(report);
            }
        }));
        (manager, fake, reports)
    }

    fn take(reports: &Reports) -> Vec<(String, PopOutChange, Option<WindowGeometry>)> {
        reports
            .lock()
            .unwrap()
            .drain(..)
            .map(|r| (r.window, r.change, r.geometry))
            .collect()
    }

    async fn settle() {
        // Paused clock: sleeping advances it once every task waits.
        tokio::time::sleep(REPORT_MAX_DELAY * 2).await;
    }

    #[tokio::test(start_paused = true)]
    async fn opening_places_titles_and_reports_where_the_window_opened() {
        let (manager, fake, reports) = manager();
        manager
            .open(&open_request("composer:d1", None), "Lunch".into())
            .unwrap();
        assert_eq!(fake.labels(), ["popout-composer:d1"]);
        assert_eq!(fake.title_of("popout-composer:d1"), "Lunch");
        {
            let windows = fake.windows.lock().unwrap();
            let spec = &windows["popout-composer:d1"].spec;
            assert_eq!(spec.url, "index.html#/window/composer:d1");
            assert_eq!(spec.min_size, MIN_POP_OUT_SIZE);
        }
        let placed = fake.geometry_of("popout-composer:d1");
        assert_eq!(placed.rect, rect(380, 240, 720, 520));
        assert!(take(&reports).is_empty(), "nothing before the move settles");
        settle().await;
        assert_eq!(
            take(&reports),
            [("composer:d1".into(), PopOutChange::Moved, Some(placed))]
        );
        // A second composer cascades; opening an open one focuses it.
        manager
            .open(&open_request("composer:d2", None), "Draft".into())
            .unwrap();
        assert_eq!(
            fake.geometry_of("popout-composer:d2").rect,
            rect(404, 264, 720, 520)
        );
        manager
            .open(&open_request("composer:d1", None), "Lunch".into())
            .unwrap();
        assert_eq!(
            fake.windows.lock().unwrap()["popout-composer:d1"].focus_count,
            2
        );
        assert_eq!(manager.open_windows(), ["composer:d1", "composer:d2"]);
    }

    #[tokio::test(start_paused = true)]
    async fn moves_are_reported_once_they_end() {
        let (manager, fake, reports) = manager();
        manager
            .open(&open_request("settings", None), "Settings".into())
            .unwrap();
        settle().await;
        take(&reports);
        // A drag: many moves, one report with the final place.
        for step in 0..5 {
            fake.user_moves("popout-settings", rect(100 + step, 100, 720, 520), false);
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        settle().await;
        assert_eq!(
            take(&reports),
            [(
                "settings".into(),
                PopOutChange::Moved,
                Some(WindowGeometry::at(rect(104, 100, 720, 520)))
            )]
        );
        // A long drag reports at most every 500 ms.
        for step in 0..12 {
            fake.user_moves("popout-settings", rect(200 + step, 100, 720, 520), false);
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        settle().await;
        let moved = take(&reports);
        assert!((2..=4).contains(&moved.len()), "{moved:?}");
        assert_eq!(
            moved.last().unwrap().2,
            Some(WindowGeometry::at(rect(211, 100, 720, 520)))
        );
        // Maximized: reported with the restored rectangle.
        fake.user_moves("popout-settings", rect(-8, -8, 1936, 1056), true);
        settle().await;
        assert_eq!(
            take(&reports),
            [(
                "settings".into(),
                PopOutChange::Moved,
                Some(WindowGeometry {
                    maximized: true,
                    ..WindowGeometry::at(rect(211, 100, 720, 520))
                })
            )]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn focus_and_closes_are_reported_and_core_closes_are_not() {
        let (manager, fake, reports) = manager();
        for window in ["a", "b", "c"] {
            manager
                .open(&open_request(window, None), window.into())
                .unwrap();
        }
        settle().await;
        take(&reports);
        fake.user_focuses("popout-b");
        assert_eq!(take(&reports), [("b".into(), PopOutChange::Focused, None)]);
        // The user's close button.
        fake.user_closes("popout-b");
        assert_eq!(take(&reports), [("b".into(), PopOutChange::Closed, None)]);
        // The core closes `a` (ReturnPopOut): it knows already.
        manager.close("a").unwrap();
        assert!(take(&reports).is_empty());
        assert_eq!(fake.labels(), ["popout-c"]);
        // Closing a window that is gone is fine.
        manager.close("a").unwrap();
        // Focusing one that is gone tells the core it closed.
        fake.windows.lock().unwrap().remove("popout-c");
        let err = manager.focus("c").unwrap_err();
        assert_eq!(err.code, ErrorCode::Conflict);
        assert_eq!(take(&reports), [("c".into(), PopOutChange::Closed, None)]);
        assert!(manager.open_windows().is_empty());
        // Moves of closed windows are not reported.
        settle().await;
        assert!(take(&reports).is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn the_main_window_closing_closes_every_pop_out() {
        let (manager, fake, reports) = manager();
        for window in ["composer:d1", "settings"] {
            manager
                .open(&open_request(window, None), window.into())
                .unwrap();
        }
        manager.close_all();
        assert!(fake.labels().is_empty());
        let mut closed = take(&reports);
        closed.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            closed,
            [
                ("composer:d1".into(), PopOutChange::Closed, None),
                ("settings".into(), PopOutChange::Closed, None),
            ]
        );
        assert!(manager.open_windows().is_empty());
        settle().await;
        assert!(take(&reports).is_empty(), "no late moves");
    }

    #[tokio::test(start_paused = true)]
    async fn failures_and_a_missing_app() {
        let manager = PopOutManager::new(tokio::runtime::Handle::current());
        let err = manager
            .open(&open_request("settings", None), "Settings".into())
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Unavailable);
        assert_eq!(
            manager.close("settings").unwrap_err().code,
            ErrorCode::Unavailable
        );
        manager.close_all();

        let (manager, fake, reports) = manager_with_failing_open();
        let err = manager
            .open(&open_request("settings", None), "Settings".into())
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Internal);
        assert_eq!(
            err.localized.as_ref().map(|l| l.id.as_str()),
            Some("popout-open-failed")
        );
        assert!(manager.open_windows().is_empty());
        assert!(fake.labels().is_empty());
        settle().await;
        assert!(take(&reports).is_empty());
    }

    fn manager_with_failing_open() -> (Arc<PopOutManager>, Arc<FakeWindows>, Reports) {
        let (manager, fake, reports) = manager();
        fake.fail_open
            .store(true, std::sync::atomic::Ordering::Relaxed);
        (manager, fake, reports)
    }

    #[tokio::test(start_paused = true)]
    async fn a_remembered_geometry_is_used_when_it_fits() {
        let (manager, fake, _reports) = manager();
        let remembered = WindowGeometry::at(rect(1000, 300, 640, 480));
        manager
            .open(
                &open_request("settings", Some(remembered)),
                "Settings".into(),
            )
            .unwrap();
        assert_eq!(fake.geometry_of("popout-settings"), remembered);
        let lost = WindowGeometry::at(rect(5000, 300, 640, 480));
        manager
            .open(&open_request("rules", Some(lost)), "Rules".into())
            .unwrap();
        assert_eq!(
            fake.geometry_of("popout-rules").rect,
            rect(420, 260, 640, 480)
        );
    }
}
