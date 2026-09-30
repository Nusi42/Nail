//! Plug-in placements in `layout.kdl` (M2d, ADR 0050): the manifest's
//! suggested placements (`nail_api::plugins::PluginPlacement`) turned into
//! the user's layout file with the items added — `item
//! "plugin.<id>.<command>"` in a toolbar or menu (after / before the named
//! item, else at the end), `plugin-panel "<id>.<panel>"` after / before the
//! first widget of the named kind (else at the end of the main window),
//! `bind` lines for the suggested keys (never replacing a key the user
//! bound). The core answers `Request::PreviewPluginPlacements` with that
//! text and the merge preview; the user confirms in the merge dialog.
//!
//! **Slot (WP60).** Owned by WP62, which writes the function here (format
//! preserving, like `write_back.rs`) with its tests; WP64 calls it from
//! the core.
