//! Consistency of the Windows installer sources (plan M2b WP44): the
//! bundle section of `tauri.conf.json`, Nail's copy of the Tauri NSIS
//! template, the hooks and their strings, and the app code they call.
//! The installer itself is built and exercised by
//! `scripts/build-installer.mjs` and `scripts/installer-smoke.ps1`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use nail_app_lib::AppArgs;
use nail_app_lib::launch::{FILE_EXTENSIONS, LaunchRequest, URL_SCHEMES};

fn app_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: impl AsRef<Path>) -> String {
    let path = path.as_ref();
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn config() -> serde_json::Value {
    serde_json::from_str(&read(app_dir().join("tauri.conf.json"))).unwrap()
}

fn hooks() -> String {
    read(app_dir().join("installer/hooks.nsh"))
}

fn template() -> String {
    read(app_dir().join("installer/installer.nsi"))
}

/// The body of `!macro NAME` … `!macroend`.
fn macro_body<'a>(source: &'a str, name: &str) -> &'a str {
    let start = [format!("!macro {name}\n"), format!("!macro {name} ")]
        .iter()
        .find_map(|head| source.find(head.as_str()))
        .unwrap_or_else(|| panic!("macro {name} missing"));
    let rest = &source[start..];
    &rest[..rest.find("!macroend").unwrap()]
}

#[test]
fn the_bundle_builds_a_per_user_nsis_installer() {
    let config = config();
    assert_eq!(config["productName"], "Nail");
    assert_eq!(config["mainBinaryName"], "Nail");
    let bundle = &config["bundle"];
    assert_eq!(bundle["active"], true);
    assert_eq!(bundle["targets"], serde_json::json!(["nsis"]));
    let windows = &bundle["windows"];
    assert_eq!(
        windows["webviewInstallMode"],
        serde_json::json!({ "type": "embedBootstrapper", "silent": true })
    );
    let nsis = &windows["nsis"];
    assert_eq!(nsis["installMode"], "currentUser", "no admin prompt");
    for key in ["template", "installerHooks", "installerIcon"] {
        let path = app_dir().join(nsis[key].as_str().unwrap());
        assert!(path.is_file(), "{key}: {}", path.display());
    }
    // Nail registers its types itself (hooks.nsh): Tauri's own file
    // associations would overwrite the user's default for .eml / .ics / .vcf.
    assert!(bundle.get("fileAssociations").is_none());
    assert!(config.pointer("/plugins/deep-link").is_none());
}

#[test]
fn the_template_copy_matches_the_installed_tauri_cli() {
    let template = template();
    let base = template
        .lines()
        .find_map(|l| l.strip_prefix("; NAIL-TEMPLATE-BASE: @tauri-apps/cli "))
        .expect("base version line")
        .trim()
        .to_owned();
    let lock: serde_json::Value =
        serde_json::from_str(&read(app_dir().join("../../package-lock.json"))).unwrap();
    let installed = lock["packages"]["node_modules/@tauri-apps/cli"]["version"]
        .as_str()
        .unwrap();
    assert_eq!(
        base, installed,
        "the Tauri CLI changed: copy its NSIS template again and re-apply the NAIL blocks \
         (app/src-tauri/installer/README.md)"
    );
    // The NAIL changes are all there.
    assert!(template.contains(r#"StrCpy $INSTDIR "$LOCALAPPDATA\Programs\${PRODUCTNAME}""#));
    assert!(template.contains("!insertmacro NAIL_PAGE_OPTIONS"));
    assert!(template.contains(r#"!include "${NAIL_LANGUAGE_DIR}\English.nsh""#));
    assert!(template.contains(r#"w "$(nailDeleteData)""#));
    // Tauri's hooks are still called.
    for hook in [
        "NSIS_HOOK_PREINSTALL",
        "NSIS_HOOK_POSTINSTALL",
        "NSIS_HOOK_PREUNINSTALL",
        "NSIS_HOOK_POSTUNINSTALL",
    ] {
        assert!(template.contains(&format!("!insertmacro {hook}")), "{hook}");
    }
}

#[test]
fn every_registered_scheme_and_type_is_handled_by_the_app() {
    let hooks = hooks();
    let register = macro_body(&hooks, "NAIL_REGISTER");
    let unregister = macro_body(&hooks, "NAIL_UNREGISTER");

    for scheme in URL_SCHEMES {
        assert!(
            register.contains(&format!(r#"\URLAssociations" "{scheme}""#)),
            "{scheme} not registered"
        );
        let url = if scheme == "mailto" {
            "mailto:a@example.org".to_owned()
        } else {
            format!("{scheme}://example.org/c.ics")
        };
        assert!(LaunchRequest::parse(&url).is_some(), "{scheme} not parsed");
    }
    for extension in FILE_EXTENSIONS {
        let progid = format!("${{NAIL_PROGID_{}}}", extension.to_uppercase());
        assert!(
            register.contains(&format!(r#"NAIL_FILE_PROGID "{progid}" ".{extension}""#)),
            ".{extension} not registered"
        );
        assert!(
            unregister.contains(&format!(
                r#"NAIL_REMOVE_FILE_PROGID "{progid}" ".{extension}""#
            )),
            ".{extension} not unregistered"
        );
        assert!(
            LaunchRequest::parse(&format!(r"C:\a.{extension}")).is_some(),
            ".{extension} not parsed"
        );
    }
    // Nothing registered beyond what the app understands.
    let registered: BTreeSet<&str> = register
        .lines()
        .filter_map(|l| l.split(r#"\URLAssociations" ""#).nth(1))
        .filter_map(|rest| rest.split('"').next())
        .collect();
    assert_eq!(registered, URL_SCHEMES.into_iter().collect());

    // Listed in Default apps, and removed again.
    assert!(register.contains(
        r#"WriteRegStr HKCU "${NAIL_REGISTERED_APPS}" "${PRODUCTNAME}" "${NAIL_CAPABILITIES}""#
    ));
    assert!(
        unregister.contains(r#"DeleteRegValue HKCU "${NAIL_REGISTERED_APPS}" "${PRODUCTNAME}""#)
    );
    assert!(unregister.contains(r#"DeleteRegKey HKCU "${NAIL_CAPABILITIES}""#));
    // Only per-user keys.
    assert!(
        !hooks.contains("HKLM"),
        "the per-user installer never writes HKLM"
    );
    // The user's own choice for a type is never overwritten: only
    // OpenWithProgids entries are added to the extensions.
    let file_progid = macro_body(&hooks, "NAIL_FILE_PROGID");
    assert!(!file_progid.contains(r#"\${EXT}" """#));
}

/// A URL or file name that breaks out of `"%1"` must not become an option:
/// every open command ends the options with `--`.
#[test]
fn open_commands_pass_the_target_after_the_end_of_options() {
    let hooks = hooks();
    let commands: Vec<&str> = hooks
        .lines()
        .filter(|l| l.contains(r"\shell\open\command"))
        .collect();
    assert_eq!(commands.len(), 2, "URL and file ProgIDs");
    for command in commands {
        assert!(
            command.ends_with(r#""$\"$INSTDIR\${MAINBINARYNAME}.exe$\" -- $\"%1$\"""#),
            "{command}"
        );
    }
    let parse =
        |args: &[&str]| AppArgs::from_process_args(args.iter().map(|a| (*a).to_owned()).collect());
    let injected = parse(&["--", "mailto:x", "--setup-purge-credentials"]);
    assert_eq!(injected.setup_action(), None);
    assert!(!injected.autostart);
    assert_eq!(injected.launch_requests().len(), 1);
    assert_eq!(parse(&["--", "--autostart"]).launch_requests().len(), 0);
    assert!(!parse(&["--", "--autostart"]).autostart);
}

#[test]
fn the_app_understands_every_flag_the_installer_passes() {
    let hooks = hooks();
    let mut flags = BTreeSet::new();
    for piece in hooks.split("--").skip(1) {
        let flag: String = piece
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        // Skips comment rules ("; -----").
        if flag.starts_with(|c: char| c.is_ascii_alphabetic()) {
            flags.insert(flag);
        }
    }
    assert_eq!(
        flags,
        [
            "autostart",
            "setup-path-add",
            "setup-path-remove",
            "setup-purge-credentials"
        ]
        .map(str::to_owned)
        .into_iter()
        .collect()
    );
    let parse =
        |args: &[&str]| AppArgs::from_process_args(args.iter().map(|a| (*a).to_owned()).collect());
    assert!(parse(&["--autostart"]).autostart);
    assert!(
        parse(&["--setup-path-add", r"C:\N\bin"])
            .setup_action()
            .is_some()
    );
    assert!(
        parse(&["--setup-path-remove", r"C:\N\bin"])
            .setup_action()
            .is_some()
    );
    assert!(
        parse(&["--setup-purge-credentials"])
            .setup_action()
            .is_some()
    );
}

#[test]
fn uninstall_keeps_data_unless_asked() {
    let hooks = hooks();
    let post = macro_body(&hooks, "NSIS_HOOK_POSTUNINSTALL");
    // Every recursive delete is behind the checkbox.
    let guarded = post
        .split("${If} $DeleteAppDataCheckboxState = 1")
        .nth(1)
        .expect("data removal is guarded");
    for line in post.lines().filter(|l| l.contains("RMDir /r")) {
        assert!(guarded.contains(line.trim()), "unguarded: {line}");
    }
    let pre = macro_body(&hooks, "NSIS_HOOK_PREUNINSTALL");
    let purge = pre.find("--setup-purge-credentials").unwrap();
    let guard = pre.find("${If} $DeleteAppDataCheckboxState = 1").unwrap();
    assert!(guard < purge, "credentials are only purged when asked");
    // Updates never unregister or delete anything.
    assert!(post.contains("${If} $UpdateMode <> 1"));
    assert!(pre.contains("${If} $UpdateMode <> 1"));
}

#[test]
fn every_installer_string_is_defined_once_and_used() {
    let english = read(app_dir().join("installer/languages/English.nsh"));
    let defined: Vec<&str> = english
        .lines()
        .filter_map(|l| l.strip_prefix("LangString "))
        .map(|l| {
            assert!(l.contains("${LANG_ENGLISH}"), "{l}");
            l.split_whitespace().next().unwrap()
        })
        .collect();
    let unique: BTreeSet<&str> = defined.iter().copied().collect();
    assert_eq!(unique.len(), defined.len(), "duplicate LangString");

    let sources = format!("{}\n{}", hooks(), template());
    let used: BTreeSet<&str> = sources
        .split("$(")
        .skip(1)
        .filter_map(|s| s.split(')').next())
        .filter(|name| name.starts_with("nail") && name.chars().all(|c| c.is_ascii_alphanumeric()))
        .collect();
    assert_eq!(used, unique, "used vs defined installer strings");
}

#[test]
fn the_portable_zip_uses_the_apps_marker() {
    let build = read(app_dir().join("../../scripts/lib/build.mjs"));
    assert!(build.contains(&format!(
        r#"{{ content: "", to: "{}" }}"#,
        nail_app_lib::portable::MARKER
    )));
}
