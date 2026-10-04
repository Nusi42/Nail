//! Windows and dialogs (M5c, ADR 0116): the window model, the remembered
//! window state and pop-out windows of the desktop app.
//!
//! * **Window ids.** A window *kind* is a layout window name
//!   (`window "<kind>"` in `layout.kdl`) or a feature dialog name: kebab-case,
//!   e.g. `composer`, `settings`, `pim-event-editor`. A window *id* is the
//!   kind, or `<kind>:<instance>` for one of several open windows of a kind
//!   (one composer per draft: `composer:d42`). [`WindowId`] parses both.
//! * **The window model.** [`WindowSpec`]: modal or not (only confirmations
//!   are modal), resizable, minimum and default size, whether the window may
//!   pop out into an OS window on the desktop, and whether several instances
//!   may be open ([`WindowInstances`]). Specs come from three places, the
//!   later winning field by field: [`DEFAULT_WINDOW_SPEC`] (or
//!   [`CONFIRMATION_WINDOW_SPEC`]), the feature's registration in the UI
//!   (`Feature.windows`) and the `window` node's properties in `layout.kdl`
//!   ([`WindowFrame`], `ResolvedLayout::window_frames`). Z-order and focus
//!   are UI state of the running session (the order of the open windows) and
//!   are not remembered.
//! * **Remembered state.** [`WindowState`] per window *kind*: the in-page
//!   geometry ([`WindowGeometry`]: the restored rectangle plus maximized /
//!   snapped), the pop-out geometry (screen coordinates) and whether the kind
//!   was last shown popped out. It lives in `<config-dir>/window-state.json`
//!   ([`WINDOW_STATE_FILE`], [`WindowStateFile`]) — not in `layout.kdl`,
//!   which describes structure, and not in `ui-state.json`, whose free-form
//!   keys would not be validated. Read with `GetWindowStates`, written with
//!   `SetWindowState` / `ResetWindowStates`, pushed as `WindowStateChanged`.
//! * **Pop-outs (desktop only).** `PopOutWindow` opens a window as its own OS
//!   window (a second webview on the same core connection, loading the UI at
//!   [`pop_out_route`]); `ReturnPopOut` puts it back into the page,
//!   `ClosePopOut` closes it, `FocusPopOut` raises it, `ListPopOuts` lists
//!   them ([`PopOut`]). The shell reports what the user does with the OS
//!   window (`DesktopChange::PopOut` with a [`PopOutReport`]); the core keeps
//!   the pop-out geometry per kind and pushes `PopOutChanged`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

/// The file in the config directory.
pub const WINDOW_STATE_FILE: &str = "window-state.json";
/// The file format version written by this build ([`WindowStateFile`]).
pub const WINDOW_STATE_VERSION: u32 = 1;
/// Most window kinds remembered; a new kind beyond it is refused.
pub const MAX_WINDOW_STATES: usize = 256;
/// Longest window kind.
pub const MAX_WINDOW_KIND_CHARS: usize = 64;
/// Longest instance key (the part after `:`).
pub const MAX_WINDOW_INSTANCE_CHARS: usize = 64;
/// Separates kind and instance in a window id.
pub const WINDOW_INSTANCE_SEPARATOR: char = ':';
/// Smallest width or height of a window (and of a minimum size), in CSS
/// pixels.
pub const MIN_WINDOW_EDGE: u32 = 120;
/// Largest width or height of a window, in CSS pixels.
pub const MAX_WINDOW_EDGE: u32 = 16_384;
/// Largest absolute x / y of a remembered window position.
pub const MAX_WINDOW_COORDINATE: i32 = 65_536;
/// How far a new instance is moved from the previous one (right and down),
/// in CSS pixels, so stacked composers stay distinguishable.
pub const WINDOW_CASCADE_OFFSET: u32 = 24;
/// Keyboard move / resize step (arrow keys on a focused title bar).
pub const WINDOW_KEYBOARD_STEP: u32 = 16;
/// How much of a window stays visible when the page shrinks or the window is
/// moved off an edge, in CSS pixels.
pub const WINDOW_MIN_VISIBLE: u32 = 48;
/// Most pop-out windows open at once.
pub const MAX_POP_OUTS: usize = 32;
/// Largest pop-out context (serialized JSON).
pub const MAX_POP_OUT_CONTEXT_BYTES: usize = 16 * 1024;
/// Longest pop-out window title.
pub const MAX_POP_OUT_TITLE_CHARS: usize = 256;
/// The UI route of a pop-out window: `#/window/<window id>`.
pub const POP_OUT_ROUTE_PREFIX: &str = "#/window/";
/// The prefix of a pop-out's OS window label (Tauri): `popout-<kind>` or
/// `popout-<kind>:<instance>`.
pub const POP_OUT_LABEL_PREFIX: &str = "popout-";

// ---------------------------------------------------------------------------
// Ids

/// Whether `kind` is a window kind: kebab-case (`[a-z0-9-]`, starting with a
/// letter, no `--`, not ending with `-`), at most [`MAX_WINDOW_KIND_CHARS`].
pub fn is_window_kind(kind: &str) -> bool {
    kind.len() <= MAX_WINDOW_KIND_CHARS
        && kind.starts_with(|c: char| c.is_ascii_lowercase())
        && !kind.ends_with('-')
        && !kind.contains("--")
        && kind
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Whether `key` is an instance key: `[A-Za-z0-9_-]`, 1 to
/// [`MAX_WINDOW_INSTANCE_CHARS`] characters (draft ids and UI counters fit).
pub fn is_window_instance(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= MAX_WINDOW_INSTANCE_CHARS
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// A parsed window id: `<kind>` or `<kind>:<instance>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowId<'a> {
    /// The window kind (layout window or dialog name).
    pub kind: &'a str,
    /// The instance key, `None` for the kind's single window.
    pub instance: Option<&'a str>,
}

impl<'a> WindowId<'a> {
    /// Parses a window id; `None` when it is not one.
    pub fn parse(id: &'a str) -> Option<Self> {
        let (kind, instance) = match id.split_once(WINDOW_INSTANCE_SEPARATOR) {
            Some((kind, instance)) => (kind, Some(instance)),
            None => (id, None),
        };
        (is_window_kind(kind) && instance.is_none_or(is_window_instance))
            .then_some(Self { kind, instance })
    }
}

/// The id of a window: the kind, or `<kind>:<instance>`.
pub fn window_id(kind: &str, instance: Option<&str>) -> String {
    match instance {
        Some(key) => format!("{kind}{WINDOW_INSTANCE_SEPARATOR}{key}"),
        None => kind.to_owned(),
    }
}

/// The kind of a window id (everything before `:`); the id itself when it
/// has no instance part. Does not validate.
pub fn window_kind(id: &str) -> &str {
    id.split_once(WINDOW_INSTANCE_SEPARATOR)
        .map_or(id, |(kind, _)| kind)
}

/// The UI route a pop-out window loads: `#/window/<id>` (ids need no
/// escaping: `[a-z0-9-]`, `:` and `[A-Za-z0-9_-]`).
pub fn pop_out_route(window: &str) -> String {
    format!("{POP_OUT_ROUTE_PREFIX}{window}")
}

/// The window id of a pop-out route (the page's `location.hash`), when it is
/// one with a valid id.
pub fn parse_pop_out_route(hash: &str) -> Option<&str> {
    let id = hash.strip_prefix(POP_OUT_ROUTE_PREFIX)?;
    WindowId::parse(id).map(|_| id)
}

/// The OS window label of a pop-out (`popout-<id>`; Tauri labels allow
/// `[A-Za-z0-9-/:_]`).
pub fn pop_out_label(window: &str) -> String {
    format!("{POP_OUT_LABEL_PREFIX}{window}")
}

/// The window id of a pop-out's OS window label.
pub fn parse_pop_out_label(label: &str) -> Option<&str> {
    let id = label.strip_prefix(POP_OUT_LABEL_PREFIX)?;
    WindowId::parse(id).map(|_| id)
}

// ---------------------------------------------------------------------------
// The window model

/// A size in CSS pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct WindowSize {
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
}

impl WindowSize {
    /// A size.
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    /// Parses `layout.kdl`'s `"<width>x<height>"` (e.g. `"760x600"`).
    pub fn parse(text: &str) -> Option<Self> {
        let (w, h) = text.split_once('x')?;
        let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
        if !digits(w) || !digits(h) {
            return None;
        }
        Some(Self::new(w.parse().ok()?, h.parse().ok()?))
    }

    /// Whether both edges are within [`MIN_WINDOW_EDGE`] ..=
    /// [`MAX_WINDOW_EDGE`].
    pub fn in_range(self) -> bool {
        let ok = |v: u32| (MIN_WINDOW_EDGE..=MAX_WINDOW_EDGE).contains(&v);
        ok(self.width) && ok(self.height)
    }

    /// Whether this size is at least `min` in both edges.
    pub fn covers(self, min: Self) -> bool {
        self.width >= min.width && self.height >= min.height
    }
}

impl std::fmt::Display for WindowSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}

/// A rectangle: position of the top-left corner and size, in CSS pixels
/// (page coordinates for in-page windows, screen coordinates for pop-outs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct WindowRect {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
}

labeled_enum! {
    /// Which half of the page (or screen) a snapped window fills.
    pub enum WindowSnap : "window-snap" {
        /// The left half.
        Left = "left",
        /// The right half.
        Right = "right",
    }
}

labeled_enum! {
    /// How many windows of a kind may be open at once.
    pub enum WindowInstances : "window-instances" {
        /// One; opening it again focuses it.
        Single = "single",
        /// Several, each with its own instance key (composers: one per
        /// draft).
        Multiple = "multiple",
    }
}

/// The resolved model of a window kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WindowSpec {
    /// Modal: blocks the rest of the page (and keeps Tab inside) while open.
    /// Only confirmations are modal.
    pub modal: bool,
    /// The user can resize it (the 8 handles, Shift+arrow keys, maximize,
    /// snap).
    pub resizable: bool,
    /// Smallest size.
    pub min_size: WindowSize,
    /// Size when first opened (before anything is remembered).
    pub size: WindowSize,
    /// May open as its own OS window in the desktop app.
    pub pop_out: bool,
    /// How many may be open at once.
    pub instances: WindowInstances,
}

/// Every window that does not say otherwise: non-modal, resizable,
/// 720 × 520 (at least 360 × 240), one at a time, stays in the page.
///
/// Popping out is opt-in (`pop-out=#true` in `layout.kdl`, or the feature's
/// registration): every webview has its own UI state, so only a window that
/// shows core data alone, or that hands its state over before it pops out
/// (a pop-out preparer, like the composer's draft), works in its own OS
/// window.
pub const DEFAULT_WINDOW_SPEC: WindowSpec = WindowSpec {
    modal: false,
    resizable: true,
    min_size: WindowSize::new(360, 240),
    size: WindowSize::new(720, 520),
    pop_out: false,
    instances: WindowInstances::Single,
};

/// Confirmations (the only modal windows): modal, fixed size 440 × 220,
/// never popped out.
pub const CONFIRMATION_WINDOW_SPEC: WindowSpec = WindowSpec {
    modal: true,
    resizable: false,
    min_size: WindowSize::new(320, 160),
    size: WindowSize::new(440, 220),
    pop_out: false,
    instances: WindowInstances::Single,
};

impl Default for WindowSpec {
    fn default() -> Self {
        DEFAULT_WINDOW_SPEC
    }
}

impl WindowSpec {
    /// This spec with the fields `frame` sets replaced.
    #[must_use]
    pub fn with_frame(self, frame: &WindowFrame) -> Self {
        Self {
            modal: frame.modal.unwrap_or(self.modal),
            resizable: frame.resizable.unwrap_or(self.resizable),
            min_size: frame.min_size.unwrap_or(self.min_size),
            size: frame.size.unwrap_or(self.size),
            pop_out: frame.pop_out.unwrap_or(self.pop_out),
            instances: frame.instances.unwrap_or(self.instances),
        }
        .normalized()
    }

    /// Edges clamped to [`MIN_WINDOW_EDGE`] ..= [`MAX_WINDOW_EDGE`] and the
    /// default size at least the minimum size.
    #[must_use]
    pub fn normalized(self) -> Self {
        let clamp = |v: u32| v.clamp(MIN_WINDOW_EDGE, MAX_WINDOW_EDGE);
        let min_size = WindowSize::new(clamp(self.min_size.width), clamp(self.min_size.height));
        let size = WindowSize::new(
            clamp(self.size.width).max(min_size.width),
            clamp(self.size.height).max(min_size.height),
        );
        Self {
            min_size,
            size,
            ..self
        }
    }
}

/// The window-model properties of a `window` node in `layout.kdl`
/// (`window "composer" size="760x600" min-size="480x360" modal=#false
/// resizable=#true pop-out=#true instances="multiple"`); every field is
/// optional and overrides the feature's registration and the defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct WindowFrame {
    /// `modal=`.
    pub modal: Option<bool>,
    /// `resizable=`.
    pub resizable: Option<bool>,
    /// `min-size="<w>x<h>"`.
    pub min_size: Option<WindowSize>,
    /// `size="<w>x<h>"`.
    pub size: Option<WindowSize>,
    /// `pop-out=`.
    pub pop_out: Option<bool>,
    /// `instances="single" | "multiple"`.
    pub instances: Option<WindowInstances>,
}

impl WindowFrame {
    /// Whether no property is set.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// `other`'s set fields over this frame's.
    #[must_use]
    pub fn merged(self, other: &Self) -> Self {
        Self {
            modal: other.modal.or(self.modal),
            resizable: other.resizable.or(self.resizable),
            min_size: other.min_size.or(self.min_size),
            size: other.size.or(self.size),
            pop_out: other.pop_out.or(self.pop_out),
            instances: other.instances.or(self.instances),
        }
    }

    /// Why the frame is not valid: a size out of range, or a default size
    /// below the minimum size (when both are set).
    pub fn issue(&self) -> Option<WindowFrameIssue> {
        if self.min_size.is_some_and(|s| !s.in_range()) || self.size.is_some_and(|s| !s.in_range())
        {
            return Some(WindowFrameIssue::SizeOutOfRange);
        }
        if let (Some(min), Some(size)) = (self.min_size, self.size)
            && !size.covers(min)
        {
            return Some(WindowFrameIssue::BelowMinimum);
        }
        None
    }
}

labeled_enum! {
    /// Why a `window` node's model properties are refused (`config-` errors
    /// in `core.ftl` carry these as `config-window-frame-<issue>`).
    pub enum WindowFrameIssue : "config-window-frame" {
        /// A width or height outside `MIN_WINDOW_EDGE` ..= `MAX_WINDOW_EDGE`.
        SizeOutOfRange = "size-out-of-range",
        /// `size` is smaller than `min-size`.
        BelowMinimum = "below-minimum",
    }
}

// ---------------------------------------------------------------------------
// Remembered state

/// Where a window was and how it was shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WindowGeometry {
    /// The normal (restored) rectangle; kept while the window is maximized
    /// or snapped so restoring brings it back.
    pub rect: WindowRect,
    /// Fills the page (or screen).
    #[serde(default)]
    pub maximized: bool,
    /// Fills a half of the page (or screen); never together with
    /// `maximized`.
    #[serde(default)]
    pub snap: Option<WindowSnap>,
}

impl WindowGeometry {
    /// A normal window at `rect`.
    pub const fn at(rect: WindowRect) -> Self {
        Self {
            rect,
            maximized: false,
            snap: None,
        }
    }

    /// Whether the geometry may be stored: edges 1 ..= [`MAX_WINDOW_EDGE`],
    /// coordinates within ±[`MAX_WINDOW_COORDINATE`], not maximized and
    /// snapped at once.
    pub fn is_valid(&self) -> bool {
        let edge = |v: u32| (1..=MAX_WINDOW_EDGE).contains(&v);
        let coord = |v: i32| v.unsigned_abs() <= MAX_WINDOW_COORDINATE.unsigned_abs();
        edge(self.rect.width)
            && edge(self.rect.height)
            && coord(self.rect.x)
            && coord(self.rect.y)
            && !(self.maximized && self.snap.is_some())
    }
}

/// The remembered state of a window kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct WindowState {
    /// In-page geometry (page coordinates), `null` = not remembered (the
    /// spec's size, centred or cascaded).
    pub page: Option<WindowGeometry>,
    /// Pop-out geometry (screen coordinates), `null` = not remembered.
    pub pop_out: Option<WindowGeometry>,
    /// The kind was last shown popped out (the desktop opens it as an OS
    /// window again; ignored where pop-outs are not available).
    pub popped_out: bool,
}

impl WindowState {
    /// Whether nothing is remembered.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Whether both geometries are valid ([`WindowGeometry::is_valid`]).
    pub fn is_valid(&self) -> bool {
        self.page.is_none_or(|g| g.is_valid()) && self.pop_out.is_none_or(|g| g.is_valid())
    }
}

/// Every remembered window state by kind.
pub type WindowStates = BTreeMap<String, WindowState>;

/// The contents of [`WINDOW_STATE_FILE`]:
/// `{"version": 1, "windows": {"composer": {"page": {...}, ...}}}`. A file
/// with another version, or one that does not parse, reads as empty (and is
/// replaced by the next write); a single entry with an invalid kind or
/// geometry is dropped on read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WindowStateFile {
    /// [`WINDOW_STATE_VERSION`].
    pub version: u32,
    /// State by window kind.
    #[serde(default)]
    pub windows: WindowStates,
}

impl Default for WindowStateFile {
    fn default() -> Self {
        Self {
            version: WINDOW_STATE_VERSION,
            windows: WindowStates::new(),
        }
    }
}

impl WindowStateFile {
    /// Reads a file's text: `None` when it is not this version's format.
    /// Invalid entries are dropped.
    pub fn parse(text: &str) -> Option<Self> {
        #[derive(Deserialize)]
        struct Raw {
            version: u32,
            #[serde(default)]
            windows: BTreeMap<String, Value>,
        }
        let raw: Raw = serde_json::from_str(text).ok()?;
        if raw.version != WINDOW_STATE_VERSION {
            return None;
        }
        let windows = raw
            .windows
            .into_iter()
            .filter(|(kind, _)| is_window_kind(kind))
            .filter_map(|(kind, value)| {
                let state: WindowState = serde_json::from_value(value).ok()?;
                (state.is_valid() && !state.is_empty()).then_some((kind, state))
            })
            .take(MAX_WINDOW_STATES)
            .collect();
        Some(Self {
            version: WINDOW_STATE_VERSION,
            windows,
        })
    }

    /// The file's text (pretty JSON, keys sorted).
    pub fn to_text(&self) -> String {
        // Plain data: serialization cannot fail.
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
}

labeled_enum! {
    /// Why a `SetWindowState` was refused (`invalid_request`).
    pub enum WindowStateIssue : "window-state-issue" {
        /// The window is not a window kind (an instance id is not accepted:
        /// state is kept per kind).
        InvalidWindow = "invalid-window",
        /// A size or position is out of range, or maximized and snapped
        /// are both set.
        InvalidGeometry = "invalid-geometry",
        /// There are already [`MAX_WINDOW_STATES`] kinds.
        TooManyWindows = "too-many-windows",
    }
}

/// Checks a write: `None` when `state` may be stored for `window` next to
/// `current` (`None` = forget the kind, always allowed for a valid kind).
pub fn check_window_state_write(
    current: &WindowStates,
    window: &str,
    state: Option<&WindowState>,
) -> Option<WindowStateIssue> {
    if !is_window_kind(window) {
        return Some(WindowStateIssue::InvalidWindow);
    }
    let state = state?;
    if !state.is_valid() {
        return Some(WindowStateIssue::InvalidGeometry);
    }
    if !state.is_empty() && !current.contains_key(window) && current.len() >= MAX_WINDOW_STATES {
        return Some(WindowStateIssue::TooManyWindows);
    }
    None
}

// ---------------------------------------------------------------------------
// Pop-outs

/// An open pop-out window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PopOut {
    /// The window id (`<kind>` or `<kind>:<instance>`).
    pub window: String,
    /// Its geometry on screen as last reported, `null` = not known yet.
    pub geometry: Option<WindowGeometry>,
    /// What the window needs to show its content in the new webview (e.g.
    /// `{"draft": "d42"}` for a composer; the pop-out's UI reads it from
    /// `ListPopOuts`), `null` = nothing.
    pub context: Option<Value>,
    /// The OS window has the focus.
    pub focused: bool,
}

labeled_enum! {
    /// What happened to a pop-out window (`PopOutChanged`).
    pub enum PopOutChange : "pop-out-change" {
        /// It was opened.
        Opened = "opened",
        /// It was moved or resized (geometry in the report).
        Moved = "moved",
        /// It got the focus.
        Focused = "focused",
        /// It was closed (the OS close button or `ClosePopOut`): the window
        /// is gone, not shown in the page.
        Closed = "closed",
        /// It went back into the page (`ReturnPopOut`).
        Returned = "returned",
    }
}

labeled_enum! {
    /// Why a pop-out request was refused (`invalid_request` / `conflict`).
    pub enum PopOutIssue : "pop-out-issue" {
        /// The window is not a window id.
        InvalidWindow = "invalid-window",
        /// There are already [`MAX_POP_OUTS`] pop-outs.
        TooMany = "too-many",
        /// The context is larger than [`MAX_POP_OUT_CONTEXT_BYTES`] or the
        /// title longer than [`MAX_POP_OUT_TITLE_CHARS`].
        TooLarge = "too-large",
        /// The window is not popped out (`ReturnPopOut`, `ClosePopOut`,
        /// `FocusPopOut`).
        NotOpen = "not-open",
    }
}

/// What the core asks the desktop shell to open
/// (`DesktopHost::open_pop_out`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PopOutOpen {
    /// The window id.
    pub window: String,
    /// The OS window label ([`pop_out_label`]).
    pub label: String,
    /// The UI route to load ([`pop_out_route`]).
    pub route: String,
    /// The window title as the UI showed it (already localized), `null` =
    /// the shell's default.
    pub title: Option<String>,
    /// Where to open it: the kind's remembered pop-out geometry, `null` =
    /// the shell's choice.
    pub geometry: Option<WindowGeometry>,
    /// The first size when nothing is remembered (CSS pixels; the window
    /// spec's `size`), `null` = [`DEFAULT_WINDOW_SPEC`]'s.
    #[serde(default)]
    pub size: Option<WindowSize>,
    /// The smallest size (CSS pixels; the window spec's `min_size`),
    /// `null` = the shell's minimum.
    #[serde(default)]
    pub min_size: Option<WindowSize>,
}

/// The first and smallest size of a pop-out as requested by the UI (the
/// window spec), normalized like [`WindowSpec::normalized`]: edges within
/// [`MIN_WINDOW_EDGE`] ..= [`MAX_WINDOW_EDGE`], the first size at least the
/// smallest one.
pub fn pop_out_sizes(
    size: Option<WindowSize>,
    min_size: Option<WindowSize>,
) -> (Option<WindowSize>, Option<WindowSize>) {
    let clamp = |s: WindowSize| {
        WindowSize::new(
            s.width.clamp(MIN_WINDOW_EDGE, MAX_WINDOW_EDGE),
            s.height.clamp(MIN_WINDOW_EDGE, MAX_WINDOW_EDGE),
        )
    };
    let min_size = min_size.map(clamp);
    let size = size.map(clamp).map(|s| match min_size {
        Some(min) => WindowSize::new(s.width.max(min.width), s.height.max(min.height)),
        None => s,
    });
    (size, min_size)
}

/// What the desktop shell reports about a pop-out the user acted on
/// (`DesktopChange::PopOut`). Reports for windows that are not open are
/// ignored, so the shell may report a close the core asked for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PopOutReport {
    /// The window id.
    pub window: String,
    /// What happened (`moved`, `focused`, `closed`; `opened` / `returned`
    /// are accepted too).
    pub change: PopOutChange,
    /// The geometry after a move or resize (screen coordinates). The shell
    /// reports it when a move / resize ends (or at most every 500 ms).
    pub geometry: Option<WindowGeometry>,
}

/// Checks a `PopOutWindow`: the id, the title and the context size, and the
/// number of open pop-outs (`open` = already open, so not counted).
pub fn check_pop_out(
    open_count: usize,
    already_open: bool,
    window: &str,
    title: Option<&str>,
    context: Option<&Value>,
) -> Option<PopOutIssue> {
    if WindowId::parse(window).is_none() {
        return Some(PopOutIssue::InvalidWindow);
    }
    if title.is_some_and(|t| t.chars().count() > MAX_POP_OUT_TITLE_CHARS)
        || context.is_some_and(|c| {
            serde_json::to_vec(c).map_or(true, |b| b.len() > MAX_POP_OUT_CONTEXT_BYTES)
        })
    {
        return Some(PopOutIssue::TooLarge);
    }
    if !already_open && open_count >= MAX_POP_OUTS {
        return Some(PopOutIssue::TooMany);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::labels::assert_labels;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn rect(x: i32, y: i32, width: u32, height: u32) -> WindowRect {
        WindowRect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn window_ids() {
        for good in ["composer", "pim-event-editor", "a1", "x"] {
            assert!(is_window_kind(good), "{good}");
        }
        for bad in ["", "Composer", "1x", "a-", "a--b", "a b", "a.b", "a:b"] {
            assert!(!is_window_kind(bad), "{bad}");
        }
        assert!(!is_window_kind(&"a".repeat(MAX_WINDOW_KIND_CHARS + 1)));
        assert_eq!(
            WindowId::parse("composer:d42"),
            Some(WindowId {
                kind: "composer",
                instance: Some("d42")
            })
        );
        assert_eq!(
            WindowId::parse("settings"),
            Some(WindowId {
                kind: "settings",
                instance: None
            })
        );
        assert_eq!(
            WindowId::parse("composer:Draft_1-x").unwrap().instance,
            Some("Draft_1-x")
        );
        for bad in [
            "",
            "composer:",
            ":x",
            "composer:a:b",
            "composer:a b",
            "Composer:x",
        ] {
            assert_eq!(WindowId::parse(bad), None, "{bad}");
        }
        assert_eq!(window_id("composer", Some("d1")), "composer:d1");
        assert_eq!(window_id("settings", None), "settings");
        assert_eq!(window_kind("composer:d1"), "composer");
        assert_eq!(window_kind("settings"), "settings");
    }

    #[test]
    fn pop_out_routes_and_labels() {
        assert_eq!(pop_out_route("composer:d1"), "#/window/composer:d1");
        assert_eq!(
            parse_pop_out_route("#/window/composer:d1"),
            Some("composer:d1")
        );
        assert_eq!(parse_pop_out_route("#/window/Bad"), None);
        assert_eq!(parse_pop_out_route("#/print/x"), None);
        assert_eq!(pop_out_label("settings"), "popout-settings");
        assert_eq!(parse_pop_out_label("popout-composer:x"), Some("composer:x"));
        assert_eq!(parse_pop_out_label("main"), None);
        assert_eq!(parse_pop_out_label("print-1"), None);
    }

    #[test]
    fn sizes() {
        assert_eq!(
            WindowSize::parse("760x600"),
            Some(WindowSize::new(760, 600))
        );
        for bad in [
            "", "760", "x600", "760x", "760X600", "-1x5", "7 60x600", "1x2x3",
        ] {
            assert_eq!(WindowSize::parse(bad), None, "{bad}");
        }
        assert_eq!(WindowSize::new(760, 600).to_string(), "760x600");
        assert!(WindowSize::new(MIN_WINDOW_EDGE, MAX_WINDOW_EDGE).in_range());
        assert!(!WindowSize::new(MIN_WINDOW_EDGE - 1, 500).in_range());
        assert!(!WindowSize::new(500, MAX_WINDOW_EDGE + 1).in_range());
    }

    #[test]
    fn specs_and_frames() {
        assert_eq!(WindowSpec::default(), DEFAULT_WINDOW_SPEC);
        const { assert!(!DEFAULT_WINDOW_SPEC.modal && CONFIRMATION_WINDOW_SPEC.modal) };
        for spec in [DEFAULT_WINDOW_SPEC, CONFIRMATION_WINDOW_SPEC] {
            assert_eq!(spec.normalized(), spec);
        }
        let frame = WindowFrame {
            size: Some(WindowSize::new(760, 600)),
            instances: Some(WindowInstances::Multiple),
            ..WindowFrame::default()
        };
        let spec = DEFAULT_WINDOW_SPEC.with_frame(&frame);
        assert_eq!(spec.size, WindowSize::new(760, 600));
        assert_eq!(spec.instances, WindowInstances::Multiple);
        assert_eq!(spec.min_size, DEFAULT_WINDOW_SPEC.min_size);
        // The default size never ends up below the minimum.
        let small = WindowFrame {
            min_size: Some(WindowSize::new(800, 700)),
            ..WindowFrame::default()
        };
        assert_eq!(
            DEFAULT_WINDOW_SPEC.with_frame(&small).size,
            WindowSize::new(800, 700)
        );
        let tiny = WindowSpec {
            min_size: WindowSize::new(1, 1),
            size: WindowSize::new(1, 99_999),
            ..DEFAULT_WINDOW_SPEC
        }
        .normalized();
        assert_eq!(
            tiny.min_size,
            WindowSize::new(MIN_WINDOW_EDGE, MIN_WINDOW_EDGE)
        );
        assert_eq!(tiny.size, WindowSize::new(MIN_WINDOW_EDGE, MAX_WINDOW_EDGE));

        assert!(WindowFrame::default().is_empty() && !frame.is_empty());
        let merged = frame.merged(&WindowFrame {
            modal: Some(true),
            size: Some(WindowSize::new(500, 400)),
            ..WindowFrame::default()
        });
        assert_eq!(merged.modal, Some(true));
        assert_eq!(merged.size, Some(WindowSize::new(500, 400)));
        assert_eq!(merged.instances, Some(WindowInstances::Multiple));

        assert_eq!(frame.issue(), None);
        assert_eq!(
            WindowFrame {
                size: Some(WindowSize::new(50, 400)),
                ..WindowFrame::default()
            }
            .issue(),
            Some(WindowFrameIssue::SizeOutOfRange)
        );
        assert_eq!(
            WindowFrame {
                size: Some(WindowSize::new(400, 300)),
                min_size: Some(WindowSize::new(500, 200)),
                ..WindowFrame::default()
            }
            .issue(),
            Some(WindowFrameIssue::BelowMinimum)
        );
        // Wire shape (the UI and the default-layout fixture rely on it).
        assert_eq!(
            serde_json::to_value(frame).unwrap(),
            json!({"modal": null, "resizable": null, "min_size": null,
                   "size": {"width": 760, "height": 600}, "pop_out": null,
                   "instances": "multiple"})
        );
        let back: WindowFrame = serde_json::from_value(json!({"modal": true})).unwrap();
        assert_eq!(back.modal, Some(true));
    }

    #[test]
    fn geometry_validity() {
        let g = WindowGeometry::at(rect(10, -20, 700, 500));
        assert!(g.is_valid());
        assert!(
            WindowGeometry {
                maximized: true,
                ..g
            }
            .is_valid()
        );
        assert!(
            !WindowGeometry {
                maximized: true,
                snap: Some(WindowSnap::Left),
                ..g
            }
            .is_valid()
        );
        assert!(!WindowGeometry::at(rect(0, 0, 0, 10)).is_valid());
        assert!(!WindowGeometry::at(rect(0, 0, 10, MAX_WINDOW_EDGE + 1)).is_valid());
        assert!(!WindowGeometry::at(rect(MAX_WINDOW_COORDINATE + 1, 0, 10, 10)).is_valid());
        assert!(WindowGeometry::at(rect(-MAX_WINDOW_COORDINATE, 0, 10, 10)).is_valid());
        assert!(!WindowGeometry::at(rect(i32::MIN, 0, 10, 10)).is_valid());
    }

    #[test]
    fn state_file_format() {
        let mut file = WindowStateFile::default();
        file.windows.insert(
            "composer".into(),
            WindowState {
                page: Some(WindowGeometry {
                    rect: rect(40, 30, 760, 600),
                    maximized: false,
                    snap: Some(WindowSnap::Right),
                }),
                pop_out: None,
                popped_out: true,
            },
        );
        let text = file.to_text();
        assert_eq!(
            serde_json::from_str::<Value>(&text).unwrap(),
            json!({
                "version": 1,
                "windows": {
                    "composer": {
                        "page": {"rect": {"x": 40, "y": 30, "width": 760, "height": 600},
                                 "maximized": false, "snap": "right"},
                        "pop_out": null,
                        "popped_out": true
                    }
                }
            })
        );
        assert_eq!(WindowStateFile::parse(&text), Some(file.clone()));
        // Missing fields default; bad entries are dropped one by one.
        let parsed = WindowStateFile::parse(
            r#"{"version": 1, "windows": {
                "settings": {"page": {"rect": {"x": 1, "y": 2, "width": 800, "height": 600}}},
                "Bad Kind": {"popped_out": true},
                "rules": {"page": {"rect": {"x": 1, "y": 2, "width": 0, "height": 600}}},
                "move": {"page": "nonsense"},
                "outbox": {}
            }}"#,
        )
        .unwrap();
        assert_eq!(parsed.windows.keys().collect::<Vec<_>>(), ["settings"]);
        assert_eq!(
            parsed.windows["settings"].page,
            Some(WindowGeometry::at(rect(1, 2, 800, 600)))
        );
        // Another version or no JSON object: not this format.
        assert_eq!(
            WindowStateFile::parse(r#"{"version": 2, "windows": {}}"#),
            None
        );
        assert_eq!(WindowStateFile::parse("[1, 2"), None);
        assert_eq!(WindowStateFile::parse("{}"), None);
    }

    #[test]
    fn writes_are_checked() {
        let mut current = WindowStates::new();
        let ok = WindowState {
            page: Some(WindowGeometry::at(rect(0, 0, 600, 400))),
            ..WindowState::default()
        };
        assert_eq!(
            check_window_state_write(&current, "composer", Some(&ok)),
            None
        );
        assert_eq!(
            check_window_state_write(&current, "composer:d1", Some(&ok)),
            Some(WindowStateIssue::InvalidWindow)
        );
        assert_eq!(
            check_window_state_write(&current, "Bad", None),
            Some(WindowStateIssue::InvalidWindow)
        );
        let bad = WindowState {
            pop_out: Some(WindowGeometry::at(rect(0, 0, 0, 400))),
            ..WindowState::default()
        };
        assert_eq!(
            check_window_state_write(&current, "composer", Some(&bad)),
            Some(WindowStateIssue::InvalidGeometry)
        );
        for i in 0..MAX_WINDOW_STATES {
            current.insert(format!("w{i}"), ok);
        }
        assert_eq!(
            check_window_state_write(&current, "new-window", Some(&ok)),
            Some(WindowStateIssue::TooManyWindows)
        );
        // Replacing, forgetting and an empty state are fine at the limit.
        assert_eq!(check_window_state_write(&current, "w0", Some(&ok)), None);
        assert_eq!(check_window_state_write(&current, "new-window", None), None);
        assert_eq!(
            check_window_state_write(&current, "new-window", Some(&WindowState::default())),
            None
        );
    }

    #[test]
    fn pop_out_sizes_are_normalized() {
        assert_eq!(pop_out_sizes(None, None), (None, None));
        assert_eq!(
            pop_out_sizes(
                Some(WindowSize::new(820, 720)),
                Some(WindowSize::new(520, 400))
            ),
            (
                Some(WindowSize::new(820, 720)),
                Some(WindowSize::new(520, 400))
            )
        );
        // The first size is at least the smallest; edges are clamped.
        assert_eq!(
            pop_out_sizes(
                Some(WindowSize::new(300, 999_999)),
                Some(WindowSize::new(520, 1))
            ),
            (
                Some(WindowSize::new(520, MAX_WINDOW_EDGE)),
                Some(WindowSize::new(520, MIN_WINDOW_EDGE))
            )
        );
    }

    #[test]
    fn pop_out_checks() {
        assert_eq!(
            check_pop_out(0, false, "composer:d1", Some("Hi"), None),
            None
        );
        assert_eq!(
            check_pop_out(0, false, "composer:", None, None),
            Some(PopOutIssue::InvalidWindow)
        );
        let big = json!("x".repeat(MAX_POP_OUT_CONTEXT_BYTES));
        assert_eq!(
            check_pop_out(0, false, "composer", None, Some(&big)),
            Some(PopOutIssue::TooLarge)
        );
        let title = "t".repeat(MAX_POP_OUT_TITLE_CHARS + 1);
        assert_eq!(
            check_pop_out(0, false, "composer", Some(&title), None),
            Some(PopOutIssue::TooLarge)
        );
        assert_eq!(
            check_pop_out(MAX_POP_OUTS, false, "composer", None, None),
            Some(PopOutIssue::TooMany)
        );
        assert_eq!(
            check_pop_out(MAX_POP_OUTS, true, "composer", None, None),
            None
        );
    }

    #[test]
    fn labels() {
        assert_labels(WindowSnap::ALL, WindowSnap::as_str, WindowSnap::message_id);
        assert_labels(
            WindowInstances::ALL,
            WindowInstances::as_str,
            WindowInstances::message_id,
        );
        assert_labels(
            WindowFrameIssue::ALL,
            WindowFrameIssue::as_str,
            WindowFrameIssue::message_id,
        );
        assert_labels(
            WindowStateIssue::ALL,
            WindowStateIssue::as_str,
            WindowStateIssue::message_id,
        );
        assert_labels(
            PopOutChange::ALL,
            PopOutChange::as_str,
            PopOutChange::message_id,
        );
        assert_labels(
            PopOutIssue::ALL,
            PopOutIssue::as_str,
            PopOutIssue::message_id,
        );
    }
}
