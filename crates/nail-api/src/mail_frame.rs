//! The script every mail frame runs (ADRs 0164, 0206, 0212).
//!
//! Mail HTML renders in an iframe with `sandbox="allow-scripts"` and no
//! `allow-same-origin`: the document has an **opaque** origin, so nothing in
//! it can reach the app (DOM, storage, IPC). The only script it runs is
//! [`FRAME_SCRIPT`], allowed by its hash ([`frame_script_source`]) in the
//! document's CSP and in the app's (a `srcdoc` document inherits the app's
//! policy); the sanitizer strips the mail's own scripts and handlers, and
//! the hash blocks anything that slips through.
//!
//! The same script serves both frames:
//!
//! * the local frame (`srcdoc`: the desktop app, `nail serve` on loopback),
//!   whose document the reader builds (`ui/src/theme/reader.ts`);
//! * the remote UI's sandbox origin (`nail_server::sandbox`).
//!
//! Before ADR 0212 the local frame was same-origin without scripts and the
//! reader listened on its document. `WebKit` (macOS, Linux) never runs a
//! listener on a document whose frame may not run scripts, even the
//! parent's own, so links, the text menu and the height did not work there.
//!
//! The script reads the app origin from the root element's
//! [`FRAME_PARENT_ATTRIBUTE`] (written by whoever builds the document,
//! before the mail's markup) and posts only there:
//!
//! * [`FRAME_HEIGHT_MESSAGE`] `{height}` — after `load`, after each image's
//!   `load` / `error`, on `<details>` toggles, when fonts arrive, on resize
//!   and whenever the root or body resizes; only when the height changed:
//!   the overflow when the document overflows the frame, else the root's own
//!   height, plus a horizontal scrollbar, clamped to [`MAX_FRAME_HEIGHT`].
//! * [`FRAME_LINK_MESSAGE`] `{open, href, index}` — the link
//!   (`closest("a[href]")`) under the pointer or keyboard focus when it
//!   changes (`href: null`, `index: -1` when none), and `open: true` for a
//!   click, Enter or a middle click on one. Those never navigate the frame
//!   (`preventDefault`); the reader follows the link. The numbering is the
//!   core's (`nail_mime::links`): the position among the document's
//!   `a[href]`.
//! * [`FRAME_MENU_MESSAGE`] `{x, y, button, text}` — a `contextmenu` (never
//!   the browser's menu, ADR 0184): the point in the frame, the button
//!   (`0` with `x = y = 0` for the keyboard) and the selected text.
//!
//! It accepts one message, only from its parent window and origin:
//! [`FRAME_SELECT_ALL_MESSAGE`] selects the document's body (the text
//! menu's "Select all"). It touches nothing else (an opaque origin has no
//! cookies or storage anyway). Changing one byte changes its hash; the
//! CSPs follow automatically (the desktop app's `tauri.conf.json` is pinned
//! by a test).

use std::sync::LazyLock;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use sha2::{Digest, Sha256};

pub use crate::remote_content::{FRAME_HEIGHT_MESSAGE, FRAME_LINK_MESSAGE, MAX_FRAME_HEIGHT};

/// Attribute of a mail document's root element naming the app origin the
/// frame script posts to (`<html data-nail-parent="https://…">`).
pub const FRAME_PARENT_ATTRIBUTE: &str = "data-nail-parent";

/// `type` of the message a mail frame posts for a `contextmenu`:
/// `{"type": "nail-frame-menu", "x", "y", "button", "text"}`.
pub const FRAME_MENU_MESSAGE: &str = "nail-frame-menu";

/// `type` of the message the reader posts to a mail frame to select its
/// whole text (the text menu's "Select all").
pub const FRAME_SELECT_ALL_MESSAGE: &str = "nail-frame-select-all";

/// The only script a mail document runs (see the module documentation).
pub const FRAME_SCRIPT: &str = concat!(
    "(function(){\"use strict\";",
    "var d=document.documentElement,t=d.getAttribute(\"data-nail-parent\");if(!t)return;",
    "var l=-1;",
    "function m(){var v=d.clientHeight,s=d.scrollHeight;",
    "var h=Math.min(Math.ceil((s>v?s:d.offsetHeight)+Math.max(0,innerHeight-v)),200000);",
    "if(h!==l){l=h;parent.postMessage({type:\"nail-frame-height\",height:h},t)}}",
    "addEventListener(\"load\",m);addEventListener(\"resize\",m);",
    "[\"load\",\"error\",\"toggle\"].forEach(function(e){document.addEventListener(e,m,true)});",
    "var r=typeof ResizeObserver===\"function\"?new ResizeObserver(m):null;if(r)r.observe(d);",
    "document.addEventListener(\"DOMContentLoaded\",function(){if(r&&document.body)r.observe(document.body);m()});",
    "if(document.fonts)document.fonts.ready.then(m);",
    "var c=null;",
    "function a(e){var n=e.target;return n&&n.closest?n.closest(\"a[href]\"):null}",
    "function p(n,o){parent.postMessage({type:\"nail-frame-link\",open:o,",
    "href:n?n.getAttribute(\"href\"):null,",
    "index:n?Array.prototype.indexOf.call(document.querySelectorAll(\"a[href]\"),n):-1},t)}",
    "function k(n){if(n!==c){c=n;p(n,false)}}",
    "document.addEventListener(\"click\",function(e){var n=a(e);if(n){e.preventDefault();p(n,true)}},true);",
    "document.addEventListener(\"auxclick\",function(e){var n=a(e);if(n){e.preventDefault();if(e.button===1)p(n,true)}},true);",
    "[\"mouseover\",\"focusin\"].forEach(function(y){document.addEventListener(y,function(e){k(a(e))},true)});",
    "[\"mouseout\",\"focusout\"].forEach(function(y){document.addEventListener(y,function(e){if(!e.relatedTarget)k(null)},true)});",
    "document.addEventListener(\"contextmenu\",function(e){e.preventDefault();",
    "parent.postMessage({type:\"nail-frame-menu\",x:e.clientX,y:e.clientY,button:e.button,text:String(getSelection())},t)},true);",
    "addEventListener(\"message\",function(e){var g=e.data;",
    "if(e.source===parent&&e.origin===t&&g&&g.type===\"nail-frame-select-all\"&&document.body)getSelection().selectAllChildren(document.body)})",
    "})();"
);

static FRAME_SCRIPT_SOURCE: LazyLock<String> = LazyLock::new(|| {
    let digest = Sha256::digest(FRAME_SCRIPT.as_bytes());
    format!("'sha256-{}'", STANDARD.encode(digest))
});

/// The CSP source that allows [`FRAME_SCRIPT`] (and nothing else):
/// `'sha256-<base64>'`.
pub fn frame_script_source() -> &'static str {
    &FRAME_SCRIPT_SOURCE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_frame_script_speaks_the_contract() {
        let s = FRAME_SCRIPT;
        assert!(s.contains(&format!("getAttribute(\"{FRAME_PARENT_ATTRIBUTE}\")")));
        assert!(s.contains(&format!("type:\"{FRAME_HEIGHT_MESSAGE}\"")));
        assert!(s.contains(&format!(",{MAX_FRAME_HEIGHT})")));
        // ADR 0206: links are reported, numbered like the core does, and
        // never followed by the frame itself.
        assert!(s.contains(&format!("type:\"{FRAME_LINK_MESSAGE}\"")));
        assert!(s.contains("closest(\"a[href]\")"));
        assert!(s.contains("querySelectorAll(\"a[href]\")"));
        for event in ["click", "auxclick"] {
            assert!(
                s.contains(&format!(
                    "addEventListener(\"{event}\",function(e){{var n=a(e);if(n){{e.preventDefault();"
                )),
                "{event}"
            );
        }
        for event in ["mouseover", "focusin", "mouseout", "focusout"] {
            assert!(s.contains(&format!("\"{event}\"")), "{event}");
        }
        // ADR 0212: the text menu, never the browser's.
        assert!(s.contains("addEventListener(\"contextmenu\",function(e){e.preventDefault();"));
        assert!(s.contains(&format!("type:\"{FRAME_MENU_MESSAGE}\"")));
        // The one message it accepts: from its parent window and origin.
        assert!(s.contains(&format!(
            "e.source===parent&&e.origin===t&&g&&g.type===\"{FRAME_SELECT_ALL_MESSAGE}\""
        )));
        assert_eq!(s.matches("addEventListener(\"message\"").count(), 1);
    }

    #[test]
    fn the_frame_script_posts_only_to_its_parent_and_touches_nothing_else() {
        let s = FRAME_SCRIPT;
        // Every message goes to the named parent: as many target arguments
        // as posts.
        assert_eq!(
            s.matches("parent.postMessage(").count(),
            s.matches("},t)").count()
        );
        assert!(s.contains("parent.postMessage("));
        assert!(!s.contains("\"*\""));
        // It never navigates or opens anything itself.
        for forbidden in [
            "location",
            "window.open",
            ".click()",
            "cookie",
            "Storage",
            "fetch",
            "XMLHttpRequest",
            "innerHTML",
            "eval",
            "Function(",
        ] {
            assert!(!s.contains(forbidden), "{forbidden}");
        }
        // It cannot end its own element.
        assert!(!s.contains("</"));
        let expected = STANDARD.encode(Sha256::digest(s.as_bytes()));
        assert_eq!(frame_script_source(), format!("'sha256-{expected}'"));
    }
}
