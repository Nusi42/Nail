//! Consistency of the Windows installer sources (plan M2b WP44, M4b WP112):
//! the bundle section of `tauri.conf.json` plus `windows/bundle.json`, Nail's
//! copy of the Tauri NSIS template, the hooks and their strings, the WiX
//! fragment of the MSI and its strings, the registry facts both installers
//! write (`nail_api::desktop::registry`, `nail_api::mapi::registry`), and
//! the app code they call. The installers themselves are built and
//! exercised by `scripts/build-installer.mjs`, `scripts/installer-smoke.ps1`
//! and `scripts/msi-smoke.ps1`.

#![cfg(test)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use nail_api::desktop::{
    InstallKind, InstallScope, REGISTERED_APP_NAME, WindowsArch, default_apps_settings_uri,
    registry,
};
use nail_api::mapi::{self, MAPI_DLL_FILE, mapi_dll_relative_path};
use nail_app_lib::AppArgs;
use nail_app_lib::launch::{FILE_EXTENSIONS, URL_SCHEMES, parse};

/// The MSI's upgrade code: never changes (another code makes Windows
/// install a second copy next to the first).
const UPGRADE_CODE: &str = "4B724EE5-4370-4C05-BE94-9D606D52D37B";

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
    read(app_dir().join("windows/nsis/hooks.nsh"))
}

fn template() -> String {
    read(app_dir().join("windows/nsis/installer.nsi"))
}

fn wix() -> String {
    read(app_dir().join("windows/wix/nail.wxs"))
}

/// `windows/bundle.json`: the bundle settings every release build merges
/// over tauri.conf.json (scripts/build-installer.mjs).
fn bundle_overlay() -> serde_json::Value {
    serde_json::from_str(&read(app_dir().join("windows/bundle.json"))).unwrap()
}

/// The value of attribute `name` on an XML line.
fn attr<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let head = format!(" {name}=\"");
    let start = line.find(&head)? + head.len();
    let rest = &line[start..];
    Some(&rest[..rest.find('"')?])
}

/// From the line containing `start` to the next `end` (inclusive).
fn section<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let from = source
        .find(start)
        .unwrap_or_else(|| panic!("{start} missing"));
    let rest = &source[from..];
    &rest[..rest.find(end).unwrap() + end.len()]
}

fn parse_args(args: &[&str]) -> AppArgs {
    AppArgs::from_process_args(args.iter().map(|a| (*a).to_owned()).collect())
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
fn the_bundle_builds_a_per_user_nsis_installer_and_an_msi() {
    let config = config();
    assert_eq!(config["productName"], "Nail");
    assert_eq!(config["mainBinaryName"], "Nail");
    let bundle = &config["bundle"];
    assert_eq!(bundle["active"], true);
    // `tauri build` alone makes the NSIS installer; release builds ask for
    // both (scripts/build-installer.mjs --bundles nsis,msi).
    let targets = bundle["targets"].as_array().unwrap();
    assert!(targets.contains(&serde_json::json!("nsis")), "{targets:?}");

    // The MSI: Nail's fragment and a fixed upgrade code.
    let overlay = bundle_overlay();
    let wix = &overlay["bundle"]["windows"]["wix"];
    assert_eq!(wix["upgradeCode"], UPGRADE_CODE);
    assert_eq!(wix["language"], "en-US");
    assert_eq!(
        wix["fragmentPaths"],
        serde_json::json!(["windows/wix/nail.wxs"])
    );
    assert!(app_dir().join("windows/wix/nail.wxs").is_file());
    assert_eq!(wix["featureRefs"], serde_json::json!(["NailIntegration"]));
    assert!(self::wix().contains(r#"<Feature Id="NailIntegration""#));
    // Only bundle settings: the overlay never touches the app's own config.
    assert_eq!(
        overlay.as_object().unwrap().keys().collect::<Vec<_>>(),
        vec!["bundle"]
    );

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
         (app/src-tauri/windows/nsis/README.md)"
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
        assert!(parse(&url).is_some(), "{scheme} not parsed");
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
            parse(&format!(r"C:\a.{extension}")).is_some(),
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
    // Only per-user keys: the machine hive is read (MAPI ownership) but
    // changed only through the elevated cmd.exe of the MAPI step.
    for line in hooks.lines().map(str::trim) {
        if line.starts_with("WriteReg") || line.starts_with("DeleteReg") {
            assert!(
                !line.contains("HKLM"),
                "the per-user installer writes HKLM: {line}"
            );
        }
    }
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
        .filter(|l| l.contains(r"\shell\open\command") && l.contains("WriteRegStr"))
        .collect();
    assert_eq!(commands.len(), 2, "URL and file ProgIDs");
    // The Simple MAPI client's start command takes no target at all.
    for line in hooks
        .lines()
        .filter(|l| l.contains(r"\shell\open\command]"))
    {
        assert!(!line.contains("%1"), "{line}");
    }
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
    let english = read(app_dir().join("windows/nsis/languages/English.nsh"));
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

#[test]
fn the_build_puts_the_mapi_dlls_where_nail_api_expects_them() {
    let build = read(app_dir().join("../../scripts/lib/build.mjs"));
    // `mapi/<arch>/NailMapi.dll` in the bundle = mapi_dll_relative_path.
    assert!(build.contains(&format!("return `mapi/${{arch}}/{MAPI_DLL_FILE}`;")));
    for arch in WindowsArch::ALL {
        assert_eq!(
            mapi_dll_relative_path(arch),
            format!(r"mapi\{}\{MAPI_DLL_FILE}", arch.as_str())
        );
    }
    // The channel manifests the release scripts write are the ones the app
    // reads (`UpdateChannel::manifest_path`).
    let manifests = read(app_dir().join("../../scripts/lib/update-manifest.mjs"));
    for channel in nail_api::desktop::UpdateChannel::ALL {
        assert!(
            manifests.contains(&format!("\"{}\"", channel.as_str())),
            "{channel:?}"
        );
        assert_eq!(
            channel.manifest_path(),
            format!(
                "{}/{}",
                channel.as_str(),
                nail_api::desktop::UPDATE_MANIFEST_FILE
            )
        );
    }
    assert!(manifests.contains(&format!(
        "export const MANIFEST_FILE = \"{}\";",
        nail_api::desktop::UPDATE_MANIFEST_FILE
    )));
}

#[test]
fn the_per_user_installer_writes_the_installation_facts() {
    let hooks = hooks();
    assert!(hooks.contains(&format!(
        r#"!define NAIL_INSTALL_KEY "{}""#,
        registry::INSTALL_KEY
    )));
    let facts = macro_body(&hooks, "NAIL_WRITE_INSTALL_FACTS");
    for (name, value) in [
        (registry::INSTALLER_VALUE, InstallKind::Nsis.as_str()),
        (registry::SCOPE_VALUE, InstallScope::PerUser.as_str()),
        (registry::INSTALL_DIR_VALUE, "$INSTDIR"),
        (registry::VERSION_VALUE, "${VERSION}"),
    ] {
        assert!(
            facts.contains(&format!(
                r#"WriteRegStr HKCU "${{NAIL_INSTALL_KEY}}" "{name}" "{value}""#
            )),
            "{name}"
        );
    }
    // Written on every install and every update (before the update guard).
    let post = macro_body(&hooks, "NSIS_HOOK_POSTINSTALL");
    let facts_at = post.find("!insertmacro NAIL_WRITE_INSTALL_FACTS").unwrap();
    assert!(facts_at < post.find("${If} $UpdateMode <> 1").unwrap());
    // The MAPI fact follows what the registry says after the elevated step
    // (1 only when HKLM names the protected copy and it exists).
    let register = section(&hooks, "Function NailRegisterMapi", "FunctionEnd");
    let after = &register[register.find("ExecShellWait").unwrap()..];
    let check = after.find("Call NailMapiRegisteredHere").unwrap();
    let on = after
        .find(&format!(
            r#"WriteRegDWORD HKCU "${{NAIL_INSTALL_KEY}}" "{}" 1"#,
            registry::MAPI_VALUE
        ))
        .unwrap();
    assert!(check < on && after[check..on].contains("${If} $0 = 1"));
    let registered_here = section(&hooks, "Function NailMapiRegisteredHere", "FunctionEnd");
    assert!(registered_here.contains(
        r#"${AndIf} ${FileExists} "${NAIL_MAPI_MACHINE_DIR}\mapi\${ARCH}\NailMapi.dll""#
    ));
    // A per-user install never disables its own updates (only the MSI or a
    // policy does).
    assert!(
        !hooks
            .lines()
            .any(|l| l.contains("WriteReg") && l.contains(registry::DISABLE_UPDATES_VALUE))
    );
    // Uninstall removes the facts before the now empty Software\Nail.
    let unregister = macro_body(&hooks, "NAIL_UNREGISTER");
    let facts_gone = unregister
        .find(r#"DeleteRegKey HKCU "${NAIL_INSTALL_KEY}""#)
        .unwrap();
    assert!(
        facts_gone
            < unregister
                .find(r#"DeleteRegKey /ifempty HKCU "Software\Nail""#)
                .unwrap()
    );
    // The keys the app reads.
    assert!(hooks.contains(&format!(r#"!define NAIL_RUN_KEY "{}""#, registry::RUN_KEY)));
    assert!(hooks.contains(&format!(
        r#"!define NAIL_CAPABILITIES "{}""#,
        registry::CAPABILITIES_KEY
    )));
    assert!(hooks.contains(&format!(
        r#"!define NAIL_REGISTERED_APPS "{}""#,
        registry::REGISTERED_APPLICATIONS_KEY
    )));
    assert_eq!(registry::RUN_VALUE, "Nail");
    assert_eq!(REGISTERED_APP_NAME, "Nail");
    assert!(
        default_apps_settings_uri(Some(InstallScope::PerUser)).ends_with("registeredAppUser=Nail")
    );
}

/// The `StrCpy $8 …` lines of `NailRegisterMapi` expanded with the given
/// values: the command line the elevated cmd.exe gets.
fn mapi_command(hooks: &str, instdir: &str, arch: &str, with_x86: bool) -> String {
    let function = section(hooks, "Function NailRegisterMapi", "FunctionEnd");
    let machine = r"C:\Program Files\Nail MAPI";
    let five = format!(r"{machine}\mapi\{arch}");
    let six = format!(r"{machine}\mapi\x86");
    let seven = r#"reg add "HKLM\SOFTWARE\Clients\Mail\Nail""#;
    let mut command = String::new();
    let mut in_x86 = false;
    for line in function.lines().map(str::trim) {
        if line.starts_with("${If} ${FileExists}") && line.contains(r"mapi\x86") {
            in_x86 = true;
        } else if line == "${EndIf}" {
            in_x86 = false;
        }
        let Some(value) = line
            .strip_prefix("StrCpy $8 '")
            .and_then(|l| l.strip_suffix('\''))
        else {
            continue;
        };
        if in_x86 && !with_x86 {
            continue;
        }
        command = value
            .replace("$8", &command)
            .replace("$7", seven)
            .replace("$5", &five)
            .replace("$6", &six)
            .replace("$SYSDIR", r"C:\WINDOWS\system32")
            .replace("$INSTDIR", instdir)
            .replace("${ARCH}", arch)
            .replace("${PRODUCTNAME}", "Nail");
    }
    format!(r#"/d /v:off /s /c "{command}""#)
}

/// M4b review: a machine-wide registration that named the DLL in the
/// user's profile let the user's unelevated programs plant code into
/// elevated programs and other users' sessions. The per-user installer
/// copies the DLLs to an administrators-only folder in the elevated step
/// and registers that copy; nothing elevated reads a user-writable file.
#[test]
fn the_per_user_mapi_option_registers_a_protected_copy_with_one_elevation() {
    let hooks = hooks();
    assert!(hooks.contains(&format!(
        r#"!define NAIL_MAPI_CLIENT_KEY "{}""#,
        mapi::registry::NAIL_MAIL_CLIENT_KEY
    )));
    assert!(hooks.contains(&format!(
        r#"!define NAIL_MAPI_REG_KEY "HKLM\SOFTWARE\{}""#,
        mapi::registry::NAIL_MAIL_CLIENT_KEY.trim_start_matches(r"Software\")
    )));
    // The copy lives below Program Files (administrators only), laid out as
    // nail_api expects (<dir>\mapi\<arch>\NailMapi.dll).
    assert!(hooks.contains(r#"!define NAIL_MAPI_MACHINE_DIR "$PROGRAMFILES64\Nail MAPI""#));
    let register = section(&hooks, "Function NailRegisterMapi", "FunctionEnd");
    assert!(register.contains(r#"StrCpy $5 "${NAIL_MAPI_MACHINE_DIR}\mapi\${ARCH}""#));
    assert_eq!(
        format!(r"mapi\x86\{MAPI_DLL_FILE}"),
        mapi_dll_relative_path(WindowsArch::X86)
    );
    assert!(register.contains(r#"StrCpy $6 "${NAIL_MAPI_MACHINE_DIR}\mapi\x86""#));

    // Nothing elevated reads a file the user can change after the prompt:
    // no .reg file, no plug-ins folder (%TEMP%).
    assert!(!hooks.contains("PLUGINSDIR"));
    assert!(!hooks.contains(".reg"));
    assert!(!hooks.contains(" import "));

    // Exactly two elevated steps (register, unregister), both cmd.exe with
    // the whole work on the command line.
    let elevated: Vec<&str> = hooks
        .lines()
        .filter(|l| l.contains("ExecShellWait"))
        .collect();
    assert_eq!(elevated.len(), 2, "register + unregister");
    for line in &elevated {
        assert!(
            line.contains(r#""runas" "$SYSDIR\cmd.exe" '/d /v:off /s /c ""#),
            "{line}"
        );
    }

    // The command: the copies first, then every HKLM value names the copy
    // (never $INSTDIR), both views, no start command for the per-user exe.
    let instdir = r"C:\Users\a b&c\AppData\Local\Programs\Nail";
    let x64 = mapi_command(&hooks, instdir, "x64", true);
    let copy64 = x64
        .find(&format!(
            r#"copy /b /y "{instdir}\mapi\x64\NailMapi.dll" "C:\Program Files\Nail MAPI\mapi\x64\NailMapi.dll""#
        ))
        .expect("x64 copy");
    let copy86 = x64
        .find(&format!(
            r#"copy /b /y "{instdir}\mapi\x86\NailMapi.dll" "C:\Program Files\Nail MAPI\mapi\x86\NailMapi.dll""#
        ))
        .expect("x86 copy");
    let reg64 = x64.find("/reg:64").unwrap();
    let reg32 = x64.find("/reg:32").unwrap();
    assert!(copy64 < reg64 && copy86 < reg32, "{x64}");
    for (name, view, arch) in [
        (mapi::registry::DLL_PATH_VALUE, "64", "x64"),
        (mapi::registry::DLL_PATH_EX_VALUE, "64", "x64"),
        (mapi::registry::DLL_PATH_VALUE, "32", "x86"),
        (mapi::registry::DLL_PATH_EX_VALUE, "32", "x86"),
    ] {
        let values = format!(
            "{} {}",
            mapi::registry::DLL_PATH_VALUE,
            mapi::registry::DLL_PATH_EX_VALUE
        );
        assert!(values.split(' ').any(|v| v == name));
        assert!(
            x64.contains(&format!(
                r#"(for %v in ({values}) do @reg add "HKLM\SOFTWARE\Clients\Mail\Nail" /v %v /d "C:\Program Files\Nail MAPI\mapi\{arch}\NailMapi.dll" /f /reg:{view})"#
            )),
            "{name} {view}: {x64}"
        );
        assert!(x64.contains(&format!(
            r#"reg add "HKLM\SOFTWARE\Clients\Mail\Nail" /ve /d "{}" /f /reg:{view}"#,
            mapi::registry::CLIENT_NAME
        )));
    }
    for segment in x64.split('&').filter(|s| s.contains("reg add")) {
        let index = segment.find(" /d \"").expect("a value");
        let value = &segment[index + 5..];
        assert!(
            value.starts_with(r"C:\Program Files\Nail MAPI\") || value.starts_with("Nail\""),
            "HKLM value outside the protected copy: {value}"
        );
    }
    assert!(!x64.contains(r"shell\open\command"));
    // Every path is quoted: & in the folder name stays literal.
    assert!(!x64.replace(&format!("\"{instdir}"), "").contains("a b&c"));
    // A package without the x86 DLL registers the native view only.
    let native = mapi_command(&hooks, instdir, "arm64", false);
    assert!(native.contains(r"mapi\arm64\NailMapi.dll"));
    assert!(!native.contains("/reg:32") && !native.contains(r"mapi\x86\NailMapi.dll"));
    // The longest accepted folder still fits NSIS_MAX_STRLEN (1024).
    let max: usize = hooks
        .lines()
        .find_map(|l| l.strip_prefix("!define NAIL_MAPI_MAX_DIR "))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let longest = format!(r"C:\{}", "x".repeat(max - 3));
    for arch in ["x64", "arm64"] {
        let command = mapi_command(&hooks, &longest, arch, true);
        assert!(command.len() < 1024, "{} chars: {command}", command.len());
    }
    // Folders cmd.exe would expand (%) or that do not fit are refused before
    // anything is elevated.
    let guard = register.find("Call NailHasPercent").unwrap();
    assert!(register[guard..].contains("${OrIf} $1 > ${NAIL_MAPI_MAX_DIR}"));
    assert!(guard < register.find("ExecShellWait").unwrap());
    assert!(register.contains(r#"DetailPrint "$(nailMapiPathUnsupported)""#));

    // Removal only when the registration is the protected copy or this
    // installation's own DLL (an earlier build), with a fixed command.
    let unregister = macro_body(&hooks, "NAIL_UNREGISTER_MAPI");
    let guard = unregister
        .find(r#"${If} $1 == "${NAIL_MAPI_MACHINE_DIR}\mapi\${ARCH}\NailMapi.dll""#)
        .expect("ownership check");
    assert!(unregister.contains(r#"${OrIf} $1 == "$INSTDIR\mapi\${ARCH}\NailMapi.dll""#));
    assert!(guard < unregister.find("ExecShellWait").unwrap());
    let remove = elevated[1];
    assert!(
        !remove.contains("$INSTDIR") && !remove.contains("$8"),
        "{remove}"
    );
    assert!(remove.contains(r#"delete "${NAIL_MAPI_REG_KEY}" /f /reg:64"#));
    assert!(remove.contains(r#"delete "${NAIL_MAPI_REG_KEY}" /f /reg:32"#));
    assert!(remove.contains(r#"rd /s /q "${NAIL_MAPI_MACHINE_DIR}""#));

    // Option: off by default, remembered, /MAPI=0|1; updates keep the
    // registration; uninstall removes it.
    let read_options = macro_body(&hooks, "NAIL_OPTIONS_FUNCTIONS");
    assert!(read_options.contains("StrCpy $NailMapi 0"));
    assert!(read_options.contains(&format!(
        r#"ReadRegDWORD $0 HKCU "${{NAIL_INSTALL_KEY}}" "{}""#,
        registry::MAPI_VALUE
    )));
    assert!(read_options.contains(r#"NAIL_OPTION_FROM_CMDLINE "/MAPI=" $NailMapi"#));
    let post = macro_body(&hooks, "NSIS_HOOK_POSTINSTALL");
    let guarded = post.split("${If} $UpdateMode <> 1").nth(1).unwrap();
    assert!(guarded.contains("Call NailRegisterMapi"));
    assert!(guarded.contains("!insertmacro NAIL_UNREGISTER_MAPI"));
    let pre = macro_body(&hooks, "NSIS_HOOK_PREUNINSTALL");
    let pre_guarded = pre.split("${If} $UpdateMode <> 1").nth(1).unwrap();
    assert!(pre_guarded.contains("!insertmacro NAIL_UNREGISTER_MAPI"));
    // The DLLs (the copy's source) go with the app on uninstall.
    let post_uninstall = macro_body(&hooks, "NSIS_HOOK_POSTUNINSTALL");
    for arch in WindowsArch::ALL {
        assert!(post_uninstall.contains(&format!(
            r#"Delete "$INSTDIR\{}""#,
            mapi_dll_relative_path(arch)
        )));
    }
}

#[test]
fn the_msi_writes_the_installation_facts_with_its_properties() {
    let wix = wix();
    for property in ["AUTOSTART", "ADDTOPATH", "DISABLEUPDATES", "MAPI"] {
        // Public, passed to the server side of managed installs, remembered
        // from the previous installation, the command line winning.
        assert!(
            wix.contains(&format!(r#"<Property Id="{property}" Secure="yes">"#)),
            "{property}"
        );
        assert!(wix.contains(&format!(
            r#"<SetProperty Id="NAIL_CMD_{property}" Value="[{property}]" Before="AppSearch" Sequence="both" />"#
        )));
        assert!(wix.contains(&format!(
            r#"<SetProperty Id="{property}" Value="[NAIL_CMD_{property}]" After="AppSearch" Sequence="both">NAIL_CMD_{property}</SetProperty>"#
        )));
        assert!(wix.contains(&format!(r#"After="Set{property}" Sequence="both">"#)));
    }
    // Remembered values come from the facts key.
    let searches: Vec<&str> = wix
        .lines()
        .filter(|l| l.contains("<RegistrySearch"))
        .collect();
    assert_eq!(searches.len(), 4);
    for line in searches {
        assert_eq!(attr(line, "Root"), Some("HKLM"), "{line}");
        assert_eq!(attr(line, "Key"), Some(registry::INSTALL_KEY), "{line}");
    }
    // DISABLEUPDATES defaults to 1 (IT deploys updates); the rest to 0.
    assert!(wix.contains(r#"<Property Id="NAIL_DISABLEUPDATES" Value="1" />"#));
    for off in ["NAIL_AUTOSTART", "NAIL_ADDTOPATH", "NAIL_MAPI"] {
        assert!(
            wix.contains(&format!(r#"<Property Id="{off}" Value="0" />"#)),
            "{off}"
        );
    }

    // The facts (nail_api::desktop::registry).
    let facts = section(&wix, r#"<Component Id="NailInstallFacts""#, "</Component>");
    assert!(facts.contains(&format!(
        r#"<RegistryKey Root="HKLM" Key="{}">"#,
        registry::INSTALL_KEY
    )));
    for (name, value) in [
        (registry::INSTALLER_VALUE, InstallKind::Msi.as_str()),
        (registry::SCOPE_VALUE, InstallScope::PerMachine.as_str()),
        (registry::INSTALL_DIR_VALUE, "[INSTALLDIR]"),
        (registry::VERSION_VALUE, "[ProductVersion]"),
        (registry::DISABLE_UPDATES_VALUE, "[NAIL_DISABLEUPDATES]"),
        (registry::MAPI_VALUE, "[NAIL_MAPI]"),
    ] {
        let line = facts
            .lines()
            .find(|l| attr(l, "Name") == Some(name))
            .unwrap_or_else(|| panic!("{name} missing"));
        assert_eq!(attr(line, "Value"), Some(value), "{name}");
    }
    for dword in [registry::DISABLE_UPDATES_VALUE, registry::MAPI_VALUE] {
        let line = facts
            .lines()
            .find(|l| attr(l, "Name") == Some(dword))
            .unwrap();
        assert_eq!(attr(line, "Type"), Some("integer"), "{dword} is a DWORD");
    }

    // AUTOSTART: the machine Run key with the app's flag.
    let run = section(&wix, r#"<Component Id="NailAutostart""#, "</Component>");
    assert!(run.contains(r#"NAIL_AUTOSTART = "1""#));
    assert!(run.contains(&format!(
        r#"Key="{}" Name="{}" Type="string" Value="&quot;[INSTALLDIR]Nail.exe&quot; --autostart""#,
        registry::RUN_KEY,
        registry::RUN_VALUE
    )));
    assert!(parse_args(&["--autostart"]).autostart);
    // ADDTOPATH: only bin\ on the machine PATH, removed on uninstall.
    let path = section(&wix, r#"<Component Id="NailAddToPath""#, "</Component>");
    assert!(path.contains(r#"NAIL_ADDTOPATH = "1""#));
    assert!(path.contains(
        r#"Name="PATH" Value="[INSTALLDIR]bin" Permanent="no" Part="last" Action="set" System="yes""#
    ));
}

#[test]
fn the_msi_registers_the_same_types_as_the_per_user_installer() {
    let wix = wix();
    let registration = section(&wix, r#"<Component Id="NailRegistration""#, "</Component>");
    for line in registration.lines().filter(|l| l.contains(" Root=")) {
        assert_eq!(attr(line, "Root"), Some("HKLM"), "{line}");
    }
    // Same schemes and extensions as the app parses and NSIS registers.
    let schemes: BTreeSet<&str> = registration
        .lines()
        .filter(|l| attr(l, "Key") == Some("URLAssociations"))
        .filter_map(|l| attr(l, "Name"))
        .collect();
    assert_eq!(schemes, URL_SCHEMES.into_iter().collect());
    let extensions: BTreeSet<&str> = registration
        .lines()
        .filter(|l| attr(l, "Key") == Some("FileAssociations"))
        .filter_map(|l| attr(l, "Name"))
        .map(|e| e.trim_start_matches('.'))
        .collect();
    assert_eq!(extensions, FILE_EXTENSIONS.into_iter().collect());
    for extension in FILE_EXTENSIONS {
        // "Open with" only; the extension's default is never touched.
        assert!(registration.contains(&format!(
            r#"Key="Software\Classes\.{extension}\OpenWithProgids" Name="Nail.{extension}""#
        )));
        assert!(!registration.contains(&format!(r#"Key="Software\Classes\.{extension}">"#)));
    }
    // Every open command ends the options with `--`.
    let commands: Vec<&str> = registration
        .lines()
        .filter(|l| attr(l, "Key") == Some(r"shell\open\command"))
        .collect();
    assert_eq!(commands.len(), 5, "mailto, webcal, eml, ics, vcf");
    for line in commands {
        assert_eq!(
            attr(line, "Value"),
            Some("&quot;[INSTALLDIR]Nail.exe&quot; -- &quot;%1&quot;"),
            "{line}"
        );
    }
    // Listed under RegisteredApplications → Settings > Default apps
    // (registeredAppMachine for this scope).
    assert!(registration.contains(&format!(
        r#"Key="{}" Name="{REGISTERED_APP_NAME}" Type="string" Value="{}""#,
        registry::REGISTERED_APPLICATIONS_KEY,
        registry::CAPABILITIES_KEY
    )));
    assert!(registration.contains(&format!(
        r#"<RegistryKey Root="HKLM" Key="{}">"#,
        registry::CAPABILITIES_KEY
    )));
    assert!(
        default_apps_settings_uri(Some(InstallScope::PerMachine))
            .ends_with("registeredAppMachine=Nail")
    );
    // The same ProgIDs as the per-user installer.
    let hooks = hooks();
    for progid in [
        "Nail.Url.mailto",
        "Nail.Url.webcal",
        "Nail.eml",
        "Nail.ics",
        "Nail.vcf",
    ] {
        assert!(hooks.contains(&format!(r#""{progid}""#)), "{progid} (NSIS)");
        assert!(
            registration.contains(&format!(r#"Key="Software\Classes\{progid}">"#)),
            "{progid} (MSI)"
        );
    }
}

#[test]
fn the_msi_mapi_component_registers_both_views_on_request() {
    let wix = wix();
    let native = section(&wix, r#"<Component Id="NailMapiNative""#, "</Component>");
    let wow = section(&wix, r#"<Component Id="NailMapiWow""#, "</Component>");
    assert!(!native.lines().next().unwrap().contains("Win64="));
    assert!(wow.lines().next().unwrap().contains(r#"Win64="no""#));
    for (component, dll) in [
        (
            native,
            format!(r"[INSTALLDIR]mapi\$(var.NailArch)\{MAPI_DLL_FILE}"),
        ),
        (
            wow,
            format!("[INSTALLDIR]{}", mapi_dll_relative_path(WindowsArch::X86)),
        ),
    ] {
        assert!(component.contains(r#"NAIL_MAPI = "1""#));
        assert!(component.contains(&format!(
            r#"<RegistryKey Root="HKLM" Key="{}">"#,
            mapi::registry::NAIL_MAIL_CLIENT_KEY
        )));
        for value in [
            mapi::registry::DLL_PATH_VALUE,
            mapi::registry::DLL_PATH_EX_VALUE,
        ] {
            let line = component
                .lines()
                .find(|l| attr(l, "Name") == Some(value))
                .unwrap_or_else(|| panic!("{value} missing"));
            assert_eq!(attr(line, "Value"), Some(dll.as_str()), "{value}");
        }
        assert!(component.contains(&format!(
            r#"Value="{}" KeyPath="yes""#,
            mapi::registry::CLIENT_NAME
        )));
    }
    // $(var.NailArch) is the package's architecture, as nail_api names it.
    for arch in [WindowsArch::X64, WindowsArch::Arm64] {
        assert!(wix.contains(&format!(r#"<?define NailArch = "{}" ?>"#, arch.as_str())));
    }
    // The installer never makes Nail the default client (Clients\Mail's
    // default value): the user chooses.
    assert!(!wix.contains(&format!(r#"Key="{}">"#, mapi::registry::MAIL_CLIENTS_KEY)));
}

#[test]
fn every_msi_component_is_in_the_feature_and_has_its_own_guid() {
    let wix = wix();
    let components: Vec<&str> = wix
        .lines()
        .filter(|l| l.trim_start().starts_with("<Component "))
        .collect();
    let ids: BTreeSet<&str> = components.iter().filter_map(|l| attr(l, "Id")).collect();
    let guids: BTreeSet<&str> = components.iter().filter_map(|l| attr(l, "Guid")).collect();
    assert_eq!(ids.len(), components.len());
    assert_eq!(guids.len(), components.len(), "unique fixed GUIDs");
    for guid in &guids {
        assert_eq!(guid.len(), 36, "{guid}");
    }
    assert!(!guids.contains(UPGRADE_CODE));
    let feature = section(&wix, r#"<Feature Id="NailIntegration""#, "</Feature>");
    let refs: BTreeSet<&str> = feature
        .lines()
        .filter(|l| l.contains("<ComponentRef"))
        .filter_map(|l| attr(l, "Id"))
        .collect();
    assert_eq!(refs, ids);
}

#[test]
fn every_msi_string_is_defined_once_and_used_and_matches_nsis() {
    let strings = read(app_dir().join("windows/wix/languages/en-US.wxi"));
    let mut defined = BTreeMap::new();
    for line in strings.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("<?define ") {
            let (name, value) = rest.split_once(" = ").unwrap();
            let value = value.trim_end_matches("?>").trim().trim_matches('"');
            assert!(
                defined.insert(name.to_owned(), value.to_owned()).is_none(),
                "duplicate {name}"
            );
        }
    }
    let wix = wix();
    assert!(wix.contains(r"<?include languages\en-US.wxi ?>"));
    let used: BTreeSet<String> = wix
        .split("$(var.")
        .skip(1)
        .filter_map(|s| s.split(')').next())
        .filter(|name| *name != "NailArch")
        .map(str::to_owned)
        .collect();
    assert_eq!(used, defined.keys().cloned().collect::<BTreeSet<_>>());
    // The registration texts read the same in both installers.
    let english = read(app_dir().join("windows/nsis/languages/English.nsh"));
    for (wix_name, nsis_name) in [
        ("NailAppDescription", "nailAppDescription"),
        ("NailMailtoName", "nailMailtoName"),
        ("NailWebcalName", "nailWebcalName"),
        ("NailEmlName", "nailEmlName"),
        ("NailIcsName", "nailIcsName"),
        ("NailVcfName", "nailVcfName"),
    ] {
        let value = &defined[wix_name];
        assert!(
            english.contains(&format!(
                r#"LangString {nsis_name} ${{LANG_ENGLISH}} "{value}""#
            )),
            "{wix_name} differs from {nsis_name}"
        );
    }
}
