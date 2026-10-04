//! Windows (M5c, ADR 0116): the remembered window state
//! (`GetWindowStates` / `SetWindowState` / `ResetWindowStates` over
//! `<config-dir>/window-state.json`, pushed as `WindowStateChanged`) and the
//! desktop app's pop-out windows (`ListPopOuts`, `PopOutWindow`,
//! `ReturnPopOut`, `ClosePopOut`, `FocusPopOut`, the shell's
//! `DesktopChange::PopOut` reports, pushed as `PopOutChanged`).
//!
//! The core keeps the list of open pop-outs (`Inner::pop_outs`) so every
//! webview — the main window and each pop-out — sees the same list and a
//! pop-out's UI can read its context. The shell only opens, closes and
//! focuses OS windows and reports what the user does with them.

use std::sync::Arc;

use nail_api::windows::{
    PopOut, PopOutChange, PopOutIssue, PopOutOpen, PopOutReport, WindowGeometry, WindowSize,
    WindowState, check_pop_out, pop_out_label, pop_out_route, pop_out_sizes, window_kind,
};
use nail_api::{ApiError, DesktopHost, ErrorCode, Event, LocalizedMessage, Request, Response};
use nail_config::{ConfigManager, WindowStateError};

use crate::error::Result;
use crate::service::Service;

/// The window-state and pop-out requests.
pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    match request {
        Request::GetWindowStates | Request::SetWindowState { .. } | Request::ResetWindowStates => {
            state_request(service, request).await
        }
        Request::ListPopOuts => Ok(pop_outs_response(service)),
        Request::PopOutWindow {
            window,
            title,
            context,
            size,
            min_size,
        } => {
            pop_out(
                service,
                window,
                title,
                context,
                pop_out_sizes(size, min_size),
            )
            .await
        }
        Request::ReturnPopOut { window } => {
            end_pop_out(service, window, PopOutChange::Returned).await
        }
        Request::ClosePopOut { window } => end_pop_out(service, window, PopOutChange::Closed).await,
        Request::FocusPopOut { window } => focus_pop_out(service, window),
        other => Err(ApiError::internal(format!(
            "{} is not a window request",
            crate::service::request_name(&other)
        ))
        .into()),
    }
}

async fn state_request(service: &Service, request: Request) -> Result<Response> {
    let config = service.config().clone();
    let events = service.inner.events.clone();
    // The config manager reads and writes a file.
    tokio::task::spawn_blocking(move || -> Result<Response> {
        let states = match request {
            Request::GetWindowStates => config.window_states(),
            Request::SetWindowState { window, state } => {
                match config.set_window_state(&window, state) {
                    Ok((states, changed)) => {
                        if changed {
                            let state = states.get(&window).copied();
                            let _ = events.send(Event::WindowStateChanged { window, state });
                        }
                        states
                    }
                    Err(err) => return Err(state_error(&window, err)),
                }
            }
            Request::ResetWindowStates => {
                let removed = config
                    .reset_window_states()
                    .map_err(|err| state_error("*", err))?;
                for window in removed {
                    let _ = events.send(Event::WindowStateChanged {
                        window,
                        state: None,
                    });
                }
                config.window_states()
            }
            _ => unreachable!("state_request gets state requests only"),
        };
        Ok(Response::WindowStates { states })
    })
    .await?
}

fn state_error(window: &str, err: WindowStateError) -> crate::error::CoreError {
    match err {
        WindowStateError::Invalid(issue) => {
            ApiError::invalid_request(format!("window state `{window}`: {}", issue.as_str()))
                .with_localized(LocalizedMessage::new(issue.message_id()))
                .into()
        }
        err @ WindowStateError::Io(_) => ApiError::internal(err.to_string()).into(),
    }
}

fn issue_error(code: ErrorCode, window: &str, issue: PopOutIssue) -> crate::error::CoreError {
    ApiError::new(code, format!("pop-out `{window}`: {}", issue.as_str()))
        .with_localized(LocalizedMessage::new(issue.message_id()))
        .into()
}

fn host(service: &Service) -> Result<Arc<dyn DesktopHost>> {
    service.inner.desktop.clone().ok_or_else(|| {
        ApiError::unsupported("only the desktop app has pop-out windows")
            .with_localized(LocalizedMessage::new("desktop-unavailable"))
            .into()
    })
}

fn snapshot(service: &Service) -> Vec<PopOut> {
    service
        .inner
        .pop_outs
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

fn pop_outs_response(service: &Service) -> Response {
    Response::PopOuts {
        pop_outs: snapshot(service),
    }
}

fn changed(service: &Service, window: &str, change: PopOutChange) {
    service.broadcast(Event::PopOutChanged {
        window: window.to_owned(),
        change,
        pop_outs: snapshot(service),
    });
}

/// Marks `window` focused (and every other pop-out not); whether it is open.
fn set_focused(service: &Service, window: &str) -> bool {
    let mut list = service
        .inner
        .pop_outs
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let found = list.iter().any(|p| p.window == window);
    if found {
        for pop_out in list.iter_mut() {
            pop_out.focused = pop_out.window == window;
        }
    }
    found
}

fn is_open(service: &Service, window: &str) -> bool {
    snapshot(service).iter().any(|p| p.window == window)
}

/// Changes the remembered state of `window`'s kind and pushes
/// `WindowStateChanged` when it changed (failures are logged: pop-outs work
/// without a writable config directory).
fn remember(
    config: &ConfigManager,
    service: &Service,
    window: &str,
    edit: impl FnOnce(&mut WindowState),
) {
    let kind = window_kind(window);
    match config.update_window_state(kind, edit) {
        Ok(Some(state)) => service.broadcast(Event::WindowStateChanged {
            window: kind.to_owned(),
            state: (!state.is_empty()).then_some(state),
        }),
        Ok(None) => {}
        Err(err) => tracing::warn!(%err, window, "cannot remember the pop-out state"),
    }
}

async fn pop_out(
    service: &Service,
    window: String,
    title: Option<String>,
    context: Option<serde_json::Value>,
    (size, min_size): (Option<WindowSize>, Option<WindowSize>),
) -> Result<Response> {
    let host = host(service)?;
    let already_open = is_open(service, &window);
    let open_count = snapshot(service).len();
    if let Some(issue) = check_pop_out(
        open_count,
        already_open,
        &window,
        title.as_deref(),
        context.as_ref(),
    ) {
        return Err(issue_error(ErrorCode::InvalidRequest, &window, issue));
    }
    if already_open {
        host.focus_pop_out(&window)?;
        set_focused(service, &window);
        changed(service, &window, PopOutChange::Focused);
        return Ok(pop_outs_response(service));
    }
    let config = service.config().clone();
    let kind = window_kind(&window).to_owned();
    let geometry: Option<WindowGeometry> = tokio::task::spawn_blocking(move || {
        config.window_states().get(&kind).and_then(|s| s.pop_out)
    })
    .await?;
    host.open_pop_out(&PopOutOpen {
        window: window.clone(),
        label: pop_out_label(&window),
        route: pop_out_route(&window),
        title,
        geometry,
        size,
        min_size,
    })?;
    {
        let mut list = service
            .inner
            .pop_outs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for other in list.iter_mut() {
            other.focused = false;
        }
        list.push(PopOut {
            window: window.clone(),
            geometry,
            context,
            focused: true,
        });
    }
    let config = service.config().clone();
    remember(&config, service, &window, |s| s.popped_out = true);
    changed(service, &window, PopOutChange::Opened);
    Ok(pop_outs_response(service))
}

async fn end_pop_out(service: &Service, window: String, change: PopOutChange) -> Result<Response> {
    let host = host(service)?;
    if !is_open(service, &window) {
        return Err(issue_error(
            ErrorCode::Conflict,
            &window,
            PopOutIssue::NotOpen,
        ));
    }
    host.close_pop_out(&window)?;
    finish(service, &window, change);
    Ok(pop_outs_response(service))
}

/// Removes a pop-out from the list (closed or returned), remembers that a
/// returned kind is shown in the page again and pushes the change.
fn finish(service: &Service, window: &str, change: PopOutChange) {
    service
        .inner
        .pop_outs
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .retain(|p| p.window != window);
    if change == PopOutChange::Returned {
        let config = service.config().clone();
        remember(&config, service, window, |s| s.popped_out = false);
    }
    changed(service, window, change);
}

fn focus_pop_out(service: &Service, window: String) -> Result<Response> {
    let host = host(service)?;
    if !is_open(service, &window) {
        return Err(issue_error(
            ErrorCode::Conflict,
            &window,
            PopOutIssue::NotOpen,
        ));
    }
    host.focus_pop_out(&window)?;
    set_focused(service, &window);
    changed(service, &window, PopOutChange::Focused);
    Ok(pop_outs_response(service))
}

/// A report of the desktop shell about a pop-out the user acted on
/// (`DesktopChange::PopOut`). Reports for windows that are not open are
/// ignored (e.g. the close of a window the core closed itself).
pub(crate) fn pop_out_reported(service: &Service, report: PopOutReport) {
    let PopOutReport {
        window,
        change,
        geometry,
    } = report;
    if !is_open(service, &window) {
        if change != PopOutChange::Opened || check_pop_out(0, false, &window, None, None).is_some()
        {
            return;
        }
        // A window the shell opened by itself (e.g. restored at start).
        service
            .inner
            .pop_outs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(PopOut {
                window: window.clone(),
                geometry: None,
                context: None,
                focused: false,
            });
    }
    match change {
        PopOutChange::Opened | PopOutChange::Moved => {
            if let Some(geometry) = geometry.filter(WindowGeometry::is_valid) {
                if let Some(entry) = service
                    .inner
                    .pop_outs
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .iter_mut()
                    .find(|p| p.window == window)
                {
                    entry.geometry = Some(geometry);
                }
                let config = service.config().clone();
                remember(&config, service, &window, |s| s.pop_out = Some(geometry));
            }
            changed(service, &window, change);
        }
        PopOutChange::Focused => {
            set_focused(service, &window);
            changed(service, &window, change);
        }
        PopOutChange::Closed | PopOutChange::Returned => finish(service, &window, change),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use nail_api::desktop::{
        DefaultAppsStatus, DesktopChange, DesktopNotifier, DesktopSettings, HostUpdateState,
        InstallationInfo, UpdateSettings,
    };
    use nail_api::windows::{
        PopOutChange, PopOutOpen, PopOutReport, WindowGeometry, WindowRect, WindowSize, WindowState,
    };
    use nail_api::{ApiError, DesktopHost, ErrorCode, Event, Request, Response};
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use crate::service::Service;

    /// A desktop host that records the pop-out calls.
    #[derive(Default)]
    struct PopOutHost {
        calls: Mutex<Vec<String>>,
        opened: Mutex<Vec<PopOutOpen>>,
        notify: Mutex<Option<DesktopNotifier>>,
    }

    impl std::fmt::Debug for PopOutHost {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("PopOutHost")
        }
    }

    impl PopOutHost {
        fn fire(&self, report: PopOutReport) {
            let notify = self.notify.lock().unwrap().clone().expect("attached");
            notify(DesktopChange::PopOut(report));
        }
    }

    impl DesktopHost for PopOutHost {
        fn attach(&self, notify: DesktopNotifier) {
            *self.notify.lock().unwrap() = Some(notify);
        }
        fn installation(&self) -> InstallationInfo {
            InstallationInfo {
                version: "1.0.0".into(),
                kind: nail_api::desktop::InstallKind::Development,
                scope: None,
                architecture: nail_api::desktop::WindowsArch::X64,
                install_dir: None,
                updates_disabled: None,
                forced_channel: None,
                mapi_installed: false,
                webview_version: None,
            }
        }
        fn update_state(&self) -> HostUpdateState {
            HostUpdateState::default()
        }
        fn apply_update_settings(&self, _: &UpdateSettings) {}
        fn check_for_updates(&self) -> Result<(), ApiError> {
            Ok(())
        }
        fn download_update(&self) -> Result<(), ApiError> {
            Ok(())
        }
        fn install_update(&self) -> Result<(), ApiError> {
            Ok(())
        }
        fn start_with_windows(&self) -> Option<bool> {
            None
        }
        fn apply_desktop_settings(&self, _: &DesktopSettings) -> Result<(), ApiError> {
            Ok(())
        }
        fn dev_endpoint_url(&self) -> Option<String> {
            None
        }
        fn default_apps(&self) -> DefaultAppsStatus {
            DefaultAppsStatus::unregistered()
        }
        fn open_default_apps_settings(&self) -> Result<(), ApiError> {
            Ok(())
        }
        fn open_pop_out(&self, open: &PopOutOpen) -> Result<(), ApiError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("open {}", open.window));
            self.opened.lock().unwrap().push(open.clone());
            Ok(())
        }
        fn close_pop_out(&self, window: &str) -> Result<(), ApiError> {
            self.calls.lock().unwrap().push(format!("close {window}"));
            Ok(())
        }
        fn focus_pop_out(&self, window: &str) -> Result<(), ApiError> {
            self.calls.lock().unwrap().push(format!("focus {window}"));
            Ok(())
        }
    }

    fn service(
        config: &std::path::Path,
        data: &std::path::Path,
        host: Option<Arc<PopOutHost>>,
    ) -> Service {
        let mut options = crate::ServiceOptions::new(data, config);
        options.watch_config = false;
        options.start_engines = false;
        options.desktop = host.map(|h| h as Arc<dyn DesktopHost>);
        Service::open(crate::demo::demo_options(options)).unwrap()
    }

    fn geometry(x: i32) -> WindowGeometry {
        WindowGeometry::at(WindowRect {
            x,
            y: 20,
            width: 760,
            height: 600,
        })
    }

    fn states(response: Response) -> nail_api::windows::WindowStates {
        match response {
            Response::WindowStates { states } => states,
            other => panic!("unexpected {other:?}"),
        }
    }

    fn pop_outs(response: Response) -> Vec<nail_api::windows::PopOut> {
        match response {
            Response::PopOuts { pop_outs } => pop_outs,
            other => panic!("unexpected {other:?}"),
        }
    }

    fn error(response: Response) -> (ErrorCode, Option<String>) {
        match response {
            Response::Error {
                code, localized, ..
            } => (code, localized.map(|l| l.id)),
            other => panic!("unexpected {other:?}"),
        }
    }

    fn drain(rx: &mut tokio::sync::broadcast::Receiver<Event>) -> Vec<Event> {
        let mut out = Vec::new();
        while let Ok(event) = rx.try_recv() {
            if matches!(
                event,
                Event::WindowStateChanged { .. } | Event::PopOutChanged { .. }
            ) {
                out.push(event);
            }
        }
        out
    }

    #[tokio::test]
    async fn window_states_get_set_reset() {
        let data = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let service = service(config.path(), data.path(), None);
        let mut events = service.inner.events.subscribe();
        assert!(states(service.handle(Request::GetWindowStates).await).is_empty());
        let state = WindowState {
            page: Some(geometry(10)),
            ..WindowState::default()
        };
        let set = Request::SetWindowState {
            window: "composer".into(),
            state: Some(state),
        };
        assert_eq!(states(service.handle(set.clone()).await)["composer"], state);
        // Unchanged: no second event.
        states(service.handle(set).await);
        states(
            service
                .handle(Request::SetWindowState {
                    window: "settings".into(),
                    state: Some(state),
                })
                .await,
        );
        assert_eq!(
            drain(&mut events),
            [
                Event::WindowStateChanged {
                    window: "composer".into(),
                    state: Some(state)
                },
                Event::WindowStateChanged {
                    window: "settings".into(),
                    state: Some(state)
                },
            ]
        );
        assert_eq!(
            error(
                service
                    .handle(Request::SetWindowState {
                        window: "composer:d1".into(),
                        state: Some(state),
                    })
                    .await
            ),
            (
                ErrorCode::InvalidRequest,
                Some("window-state-issue-invalid-window".into())
            )
        );
        assert!(states(service.handle(Request::ResetWindowStates).await).is_empty());
        assert_eq!(
            drain(&mut events),
            [
                Event::WindowStateChanged {
                    window: "composer".into(),
                    state: None
                },
                Event::WindowStateChanged {
                    window: "settings".into(),
                    state: None
                },
            ]
        );
    }

    #[tokio::test]
    async fn pop_outs_need_the_desktop() {
        let data = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let service = service(config.path(), data.path(), None);
        assert!(pop_outs(service.handle(Request::ListPopOuts).await).is_empty());
        assert_eq!(
            error(
                service
                    .handle(Request::PopOutWindow {
                        window: "composer".into(),
                        title: None,
                        context: None,
                        size: None,
                        min_size: None,
                    })
                    .await
            ),
            (ErrorCode::Unsupported, Some("desktop-unavailable".into()))
        );
    }

    #[tokio::test]
    async fn pop_out_lifecycle() {
        let data = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let host = Arc::new(PopOutHost::default());
        let service = service(config.path(), data.path(), Some(Arc::clone(&host)));
        // A remembered pop-out geometry is where the window opens.
        service
            .handle(Request::SetWindowState {
                window: "composer".into(),
                state: Some(WindowState {
                    pop_out: Some(geometry(300)),
                    ..WindowState::default()
                }),
            })
            .await;
        let mut events = service.inner.events.subscribe();
        let list = pop_outs(
            service
                .handle(Request::PopOutWindow {
                    window: "composer:d1".into(),
                    title: Some("New message".into()),
                    context: Some(json!({"draft": "d1"})),
                    size: Some(WindowSize::new(820, 720)),
                    // Below the edge limit: clamped.
                    min_size: Some(WindowSize::new(520, 1)),
                })
                .await,
        );
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].context, Some(json!({"draft": "d1"})));
        assert!(list[0].focused);
        let opened = host.opened.lock().unwrap()[0].clone();
        assert_eq!(opened.label, "popout-composer:d1");
        assert_eq!(opened.route, "#/window/composer:d1");
        assert_eq!(opened.title.as_deref(), Some("New message"));
        assert_eq!(opened.geometry, Some(geometry(300)));
        // The window spec's sizes reach the shell (normalized).
        assert_eq!(opened.size, Some(WindowSize::new(820, 720)));
        assert_eq!(
            opened.min_size,
            Some(WindowSize::new(520, nail_api::windows::MIN_WINDOW_EDGE))
        );
        let seen = drain(&mut events);
        assert!(matches!(
            &seen[0],
            Event::WindowStateChanged { window, state: Some(s) } if window == "composer" && s.popped_out
        ));
        assert!(matches!(
            &seen[1],
            Event::PopOutChanged { change: PopOutChange::Opened, pop_outs, .. } if pop_outs.len() == 1
        ));
        // Again: focused, not opened twice.
        pop_outs(
            service
                .handle(Request::PopOutWindow {
                    window: "composer:d1".into(),
                    title: None,
                    context: None,
                    size: None,
                    min_size: None,
                })
                .await,
        );
        // A second window; the first loses the focus.
        let list = pop_outs(
            service
                .handle(Request::PopOutWindow {
                    window: "settings".into(),
                    title: None,
                    context: None,
                    size: None,
                    min_size: None,
                })
                .await,
        );
        assert_eq!(
            list.iter()
                .map(|p| (p.window.as_str(), p.focused))
                .collect::<Vec<_>>(),
            [("composer:d1", false), ("settings", true)]
        );
        // The user moves the composer: geometry kept per kind.
        host.fire(PopOutReport {
            window: "composer:d1".into(),
            change: PopOutChange::Moved,
            geometry: Some(geometry(500)),
        });
        let remembered = states(service.handle(Request::GetWindowStates).await);
        assert_eq!(remembered["composer"].pop_out, Some(geometry(500)));
        assert!(remembered["composer"].popped_out);
        drain(&mut events);
        // Return the composer into the page: popped_out forgotten.
        let list = pop_outs(
            service
                .handle(Request::ReturnPopOut {
                    window: "composer:d1".into(),
                })
                .await,
        );
        assert_eq!(list.len(), 1);
        let remembered = states(service.handle(Request::GetWindowStates).await);
        assert!(!remembered["composer"].popped_out);
        assert_eq!(remembered["composer"].pop_out, Some(geometry(500)));
        // Returning again: not open.
        assert_eq!(
            error(
                service
                    .handle(Request::ReturnPopOut {
                        window: "composer:d1".into(),
                    })
                    .await
            ),
            (ErrorCode::Conflict, Some("pop-out-issue-not-open".into()))
        );
        // The user closes the settings pop-out with the OS button; a later
        // report about it is ignored.
        host.fire(PopOutReport {
            window: "settings".into(),
            change: PopOutChange::Closed,
            geometry: None,
        });
        host.fire(PopOutReport {
            window: "settings".into(),
            change: PopOutChange::Moved,
            geometry: Some(geometry(1)),
        });
        assert!(pop_outs(service.handle(Request::ListPopOuts).await).is_empty());
        let seen = drain(&mut events);
        assert!(seen.iter().any(|e| matches!(
            e,
            Event::PopOutChanged { window, change: PopOutChange::Returned, .. } if window == "composer:d1"
        )));
        assert!(seen.iter().any(|e| matches!(
            e,
            Event::PopOutChanged { window, change: PopOutChange::Closed, .. } if window == "settings"
        )));
        assert!(!seen.iter().any(|e| matches!(
            e,
            Event::PopOutChanged { window, change: PopOutChange::Moved, .. } if window == "settings"
        )));
        // A closed kind stays "popped out" (it opens as an OS window again).
        assert!(states(service.handle(Request::GetWindowStates).await)["settings"].popped_out);
        assert_eq!(
            host.calls.lock().unwrap().clone(),
            [
                "open composer:d1",
                "focus composer:d1",
                "open settings",
                "close composer:d1"
            ]
        );
        assert_eq!(
            error(
                service
                    .handle(Request::PopOutWindow {
                        window: "Bad".into(),
                        title: None,
                        context: None,
                        size: None,
                        min_size: None,
                    })
                    .await
            ),
            (
                ErrorCode::InvalidRequest,
                Some("pop-out-issue-invalid-window".into())
            )
        );
    }
}
