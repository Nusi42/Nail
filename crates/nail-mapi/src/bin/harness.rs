//! Simple MAPI test harness (WP111; used by the DLL's tests and the M4b
//! end-to-end scenario). A developer tool, not shipped: its output is for
//! scripts, not users.
//!
//! ```text
//! nail-mapi-harness send [--dll PATH] [--ansi] [--no-ui]
//!                        [--to ADDR]... [--cc ADDR]... [--bcc ADDR]...
//!                        [--subject TEXT] [--body TEXT] [--attach PATH[|NAME]]...
//!     Loads NailMapi.dll and calls MAPISendMailW (or MAPISendMail with
//!     --ansi) like Explorer's "Send to → Mail recipient"; exits with the
//!     MAPI code.
//! nail-mapi-harness documents [--dll PATH] PATH[;PATH...]
//!     Calls MAPISendDocuments.
//! nail-mapi-harness exports [--dll PATH]
//!     Prints each Simple MAPI export and whether GetProcAddress finds it by
//!     its plain name; exits 1 when one is missing.
//! nail-mapi-harness registry INSTALL_DIR [--os x64|arm64|x86]
//!     Prints the .reg file registering NailMapi.dll for that installation.
//! nail-mapi-harness            (no arguments: fake Nail.exe)
//!     Serves NAIL_MAPI_PIPE once: reads the request and the attachments
//!     (as the app does before answering), writes them as JSON to
//!     NAIL_MAPI_HARNESS_RECORD (when set) and answers
//!     NAIL_MAPI_HARNESS_RESULT (a kebab-case MapiResult, default
//!     "success").
//! ```

use std::process::ExitCode;

#[cfg(not(windows))]
fn main() -> ExitCode {
    ExitCode::from(u8::try_from(nail_api::mapi::MapiResult::NotSupported.code()).unwrap_or(u8::MAX))
}

#[cfg(windows)]
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            eprintln!("nail-mapi-harness: {message}");
            ExitCode::from(2)
        }
    }
}

/// Environment variable: where the fake app writes what it received.
#[cfg(windows)]
const RECORD_ENV: &str = "NAIL_MAPI_HARNESS_RECORD";
/// Environment variable: what the fake app answers.
#[cfg(windows)]
const RESULT_ENV: &str = "NAIL_MAPI_HARNESS_RESULT";

#[cfg(windows)]
fn run(args: &[String]) -> Result<u8, String> {
    match args.first().map(String::as_str) {
        None | Some("serve") => serve(),
        Some("send") => send(&args[1..]),
        Some("documents") => documents(&args[1..]),
        Some("exports") => exports(&args[1..]),
        Some("registry") => registry(&args[1..]),
        Some(other) => Err(format!("unknown command {other}")),
    }
}

#[cfg(windows)]
fn default_dll() -> std::path::PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    let dir = exe
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default();
    [
        dir.join("nail_mapi.dll"),
        dir.join("deps").join("nail_mapi.dll"),
        dir.join(nail_api::mapi::MAPI_DLL_FILE),
    ]
    .into_iter()
    .find(|p| p.is_file())
    .unwrap_or_else(|| dir.join("nail_mapi.dll"))
}

#[cfg(windows)]
fn value<'a>(args: &'a [String], index: &mut usize, flag: &str) -> Result<&'a str, String> {
    *index += 1;
    args.get(*index)
        .map(String::as_str)
        .ok_or_else(|| format!("{flag} needs a value"))
}

#[cfg(windows)]
fn send(args: &[String]) -> Result<u8, String> {
    use nail_mapi::windows::caller::{CallerMessage, MapiDll};
    use windows_sys::Win32::System::Mapi::{MAPI_BCC, MAPI_CC, MAPI_DIALOG, MAPI_TO};

    let mut dll = default_dll();
    let mut ansi = false;
    let mut flags = MAPI_DIALOG;
    let mut message = CallerMessage::default();
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        match flag {
            "--dll" => dll = value(args, &mut i, flag)?.into(),
            "--ansi" => ansi = true,
            "--no-ui" => flags = 0,
            "--to" | "--cc" | "--bcc" => {
                let class = match flag {
                    "--to" => MAPI_TO,
                    "--cc" => MAPI_CC,
                    _ => MAPI_BCC,
                };
                let address = value(args, &mut i, flag)?;
                message
                    .recipients
                    .push((class, None, Some(format!("SMTP:{address}"))));
            }
            "--subject" => message.subject = Some(value(args, &mut i, flag)?.to_owned()),
            "--body" => message.body = Some(value(args, &mut i, flag)?.to_owned()),
            "--attach" => {
                let spec = value(args, &mut i, flag)?;
                let (path, name) = match spec.split_once('|') {
                    Some((path, name)) => (path, Some(name.to_owned())),
                    None => (spec, None),
                };
                message.files.push((path.to_owned(), name));
            }
            other => return Err(format!("unknown option {other}")),
        }
        i += 1;
    }
    let dll = MapiDll::load(&dll).map_err(|e| format!("cannot load {}: {e}", dll.display()))?;
    let code = if ansi {
        dll.send_mail(&message, flags)
    } else {
        dll.send_mail_w(&message, flags)
    }
    .ok_or("export missing")?;
    println!("{code}");
    Ok(u8::try_from(code).unwrap_or(u8::MAX))
}

#[cfg(windows)]
fn documents(args: &[String]) -> Result<u8, String> {
    use nail_mapi::windows::caller::MapiDll;
    let mut dll = default_dll();
    let mut paths = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--dll" => dll = value(args, &mut i, "--dll")?.into(),
            other => paths = Some(other.to_owned()),
        }
        i += 1;
    }
    let paths = paths.ok_or("no paths")?;
    let dll = MapiDll::load(&dll).map_err(|e| format!("cannot load {}: {e}", dll.display()))?;
    let code = dll
        .send_documents(";", &paths, None)
        .ok_or("export missing")?;
    println!("{code}");
    Ok(u8::try_from(code).unwrap_or(u8::MAX))
}

#[cfg(windows)]
fn exports(args: &[String]) -> Result<u8, String> {
    use nail_mapi::windows::caller::MapiDll;
    let dll = match args {
        [flag, path] if flag == "--dll" => path.into(),
        [] => default_dll(),
        _ => return Err("usage: exports [--dll PATH]".into()),
    };
    let dll = MapiDll::load(&dll).map_err(|e| format!("cannot load {}: {e}", dll.display()))?;
    let mut missing = false;
    for (name, _) in nail_mapi::EXPORTS {
        let found = dll.symbol(name).is_some();
        missing |= !found;
        println!("{name}\t{}", if found { "ok" } else { "missing" });
    }
    Ok(u8::from(missing))
}

#[cfg(windows)]
fn registry(args: &[String]) -> Result<u8, String> {
    use nail_api::desktop::WindowsArch;
    use nail_mapi::registration::{client_registration, to_reg_file};
    let dir = args
        .first()
        .ok_or("usage: registry INSTALL_DIR [--os ARCH]")?;
    let os = match args.get(1..) {
        Some([flag, arch]) if flag == "--os" => WindowsArch::ALL
            .into_iter()
            .find(|a| a.as_str() == arch)
            .ok_or_else(|| format!("unknown architecture {arch}"))?,
        Some([]) | None => WindowsArch::current().unwrap_or(WindowsArch::X64),
        _ => return Err("usage: registry INSTALL_DIR [--os ARCH]".into()),
    };
    print!("{}", to_reg_file(&client_registration(dir, os)));
    Ok(0)
}

#[cfg(windows)]
fn serve() -> Result<u8, String> {
    use nail_api::mapi::{MAPI_PIPE_ENV, MapiPipeResponse, MapiResult};
    use nail_mapi::windows::server::FakeAppPipe;

    let pipe = std::env::var(MAPI_PIPE_ENV).map_err(|_| format!("{MAPI_PIPE_ENV} is not set"))?;
    let result = match std::env::var(RESULT_ENV) {
        Ok(name) => serde_json::from_value::<MapiResult>(serde_json::Value::String(name))
            .map_err(|e| e.to_string())?,
        Err(_) => MapiResult::Success,
    };
    let mut files = Vec::new();
    let server = FakeAppPipe::create(&pipe).map_err(|e| e.to_string())?;
    let request = server
        .serve_one(|request| {
            // Like the core: read every attachment before answering.
            for attachment in &request.message.attachments {
                let content = std::fs::read(&attachment.path)
                    .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                    .ok();
                files.push(serde_json::json!({
                    "path": attachment.path,
                    "name": attachment.display_name(),
                    "content": content,
                }));
            }
            MapiPipeResponse { result }
        })
        .map_err(|e| e.to_string())?;
    if let Ok(record) = std::env::var(RECORD_ENV) {
        let json = serde_json::json!({ "request": request, "files": files });
        std::fs::write(record, json.to_string()).map_err(|e| e.to_string())?;
    }
    Ok(0)
}
