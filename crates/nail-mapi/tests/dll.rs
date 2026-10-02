//! The DLL as programs see it: loaded with `LoadLibraryW`, called through
//! `GetProcAddress`, talking to a fake app on a test pipe (`NAIL_MAPI_PIPE`).
//! The tests share the process environment, so they run one at a time.
#![cfg(windows)]
#![allow(unsafe_code)]

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use nail_api::mapi::{
    MAPI_PIPE_ENV, MapiPipeRequest, MapiPipeResponse, MapiRecipientClass, MapiResult, MapiUi,
};
use nail_mapi::windows::caller::{CallerMessage, MapiDll};
use nail_mapi::windows::client::MAPI_APP_ENV;
use nail_mapi::windows::server::{FakeAppPipe, unique_pipe_name};
use pretty_assertions::assert_eq;
use windows_sys::Win32::System::Mapi::{MAPI_CC, MAPI_DIALOG, MAPI_TO, MapiMessageW};

static ENV: Mutex<()> = Mutex::new(());

/// Holds the environment lock with `NAIL_MAPI_PIPE` (and optionally
/// `NAIL_MAPI_APP`) set; clears them on drop.
struct TestEnv {
    pipe: String,
    _lock: MutexGuard<'static, ()>,
}

impl TestEnv {
    fn new(tag: &str) -> Self {
        let lock = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let pipe = unique_pipe_name(tag);
        // SAFETY: tests touching the environment hold `ENV`.
        unsafe {
            std::env::set_var(MAPI_PIPE_ENV, &pipe);
            std::env::remove_var(MAPI_APP_ENV);
        }
        Self { pipe, _lock: lock }
    }

    fn set(&self, key: &str, value: &std::ffi::OsStr) {
        // SAFETY: the environment lock is held.
        unsafe { std::env::set_var(key, value) };
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        // SAFETY: the environment lock is still held.
        unsafe {
            for key in [
                MAPI_PIPE_ENV,
                MAPI_APP_ENV,
                "NAIL_MAPI_HARNESS_RECORD",
                "NAIL_MAPI_HARNESS_RESULT",
            ] {
                std::env::remove_var(key);
            }
        }
    }
}

fn harness() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_nail-mapi-harness"))
}

fn dll_path() -> PathBuf {
    let dir = harness().parent().unwrap().to_path_buf();
    [
        dir.join("nail_mapi.dll"),
        dir.join("deps").join("nail_mapi.dll"),
    ]
    .into_iter()
    .find(|p| p.is_file())
    .expect("nail_mapi.dll next to the harness")
}

fn dll() -> MapiDll {
    MapiDll::load(&dll_path()).unwrap()
}

/// A fake app answering `result`; returns what it received plus the
/// attachments' content read before answering (as the core does).
fn fake_app(
    pipe: &str,
    result: MapiResult,
) -> std::thread::JoinHandle<(MapiPipeRequest, Vec<Vec<u8>>)> {
    let server = FakeAppPipe::create(pipe).unwrap();
    std::thread::spawn(move || {
        let mut files = Vec::new();
        let request = server
            .serve_one(|request| {
                files = request
                    .message
                    .attachments
                    .iter()
                    .map(|a| std::fs::read(&a.path).unwrap())
                    .collect();
                MapiPipeResponse { result }
            })
            .unwrap();
        (request, files)
    })
}

#[test]
fn every_export_is_found_by_its_plain_name() {
    let dll = dll();
    for (name, _) in nail_mapi::EXPORTS {
        assert!(dll.symbol(name).is_some(), "{name} is not exported");
    }
    assert!(dll.symbol("MAPIInitialize").is_none());
}

#[test]
fn send_mail_w_opens_the_composer_with_copies_of_the_files() {
    let env = TestEnv::new("w");
    let src = tempfile::tempdir().unwrap();
    let file = src.path().join("report.txt");
    std::fs::write(&file, "quarterly numbers").unwrap();
    let app = fake_app(&env.pipe, MapiResult::Success);
    let message = CallerMessage {
        subject: Some("Q3 – Bericht".into()),
        body: Some("See attached.".into()),
        recipients: vec![
            (
                MAPI_TO,
                Some("Ada".into()),
                Some("SMTP:ada@x.example".into()),
            ),
            (MAPI_CC, Some("bob@x.example".into()), None),
        ],
        files: vec![(file.to_str().unwrap().into(), Some("Q3 report.txt".into()))],
    };
    let code = dll().send_mail_w(&message, MAPI_DIALOG).unwrap();
    assert_eq!(code, 0);
    let (request, files) = app.join().unwrap();
    let m = &request.message;
    assert_eq!(m.subject.as_deref(), Some("Q3 – Bericht"));
    assert_eq!(m.body.as_deref(), Some("See attached."));
    assert_eq!(m.ui, MapiUi::Dialog);
    assert_eq!(m.addresses(MapiRecipientClass::To), ["ada@x.example"]);
    assert_eq!(m.addresses(MapiRecipientClass::Cc), ["bob@x.example"]);
    let program = m.program.as_deref().unwrap().to_ascii_lowercase();
    assert!(
        program.starts_with("dll-") && program.ends_with(".exe"),
        "{program}"
    );
    // The app got a private copy, not the caller's file …
    let sent = PathBuf::from(&m.attachments[0].path);
    assert_ne!(sent, file);
    assert_eq!(m.attachments[0].file_name.as_deref(), Some("Q3 report.txt"));
    assert_eq!(files[0], b"quarterly numbers");
    // … which is gone once the call returned; the caller's file is untouched.
    assert!(!sent.exists());
    assert!(file.exists());
}

#[test]
fn ansi_send_mail_and_the_apps_answer() {
    let env = TestEnv::new("a");
    let app = fake_app(&env.pipe, MapiResult::LoginFailure);
    let message = CallerMessage {
        subject: Some("Invoice".into()),
        recipients: vec![(MAPI_TO, None, Some("carol@x.example".into()))],
        ..Default::default()
    };
    let code = dll().send_mail(&message, 0).unwrap();
    assert_eq!(code, MapiResult::LoginFailure.code());
    let (request, _) = app.join().unwrap();
    assert_eq!(request.message.subject.as_deref(), Some("Invoice"));
    assert_eq!(request.message.ui, MapiUi::None);
}

#[test]
fn send_documents_attaches_the_files() {
    let env = TestEnv::new("docs");
    let src = tempfile::tempdir().unwrap();
    let a = src.path().join("a.txt");
    let b = src.path().join("b.txt");
    std::fs::write(&a, "A").unwrap();
    std::fs::write(&b, "B").unwrap();
    let app = fake_app(&env.pipe, MapiResult::Success);
    let paths = format!("{};{}", a.display(), b.display());
    assert_eq!(
        dll().send_documents(";", &paths, Some("first.txt;")),
        Some(0)
    );
    let (request, files) = app.join().unwrap();
    assert_eq!(request.message.attachments.len(), 2);
    assert_eq!(request.message.attachments[0].display_name(), "first.txt");
    assert_eq!(request.message.attachments[1].display_name(), "b.txt");
    assert_eq!(files, [b"A".to_vec(), b"B".to_vec()]);
}

#[test]
fn invalid_input_is_answered_without_the_app() {
    let _env = TestEnv::new("invalid");
    let dll = dll();
    // No server exists: anything reaching the pipe would fail differently.
    let bad_recipient = CallerMessage {
        recipients: vec![(MAPI_TO, Some("Nobody".into()), None)],
        ..Default::default()
    };
    assert_eq!(
        dll.send_mail_w(&bad_recipient, 0),
        Some(MapiResult::InvalidRecips.code())
    );
    let missing = CallerMessage {
        files: vec![(r"C:\does\not\exist.txt".into(), None)],
        ..Default::default()
    };
    assert_eq!(
        dll.send_mail_w(&missing, 0),
        Some(MapiResult::AttachmentNotFound.code())
    );
    let bad_class = CallerMessage {
        recipients: vec![(9, None, Some("a@x.example".into()))],
        ..Default::default()
    };
    assert_eq!(
        dll.send_mail_w(&bad_class, 0),
        Some(MapiResult::BadRecipType.code())
    );
    // Bad pointers: refused, the process survives.
    // SAFETY: the DLL validates the pointer.
    unsafe {
        assert_eq!(
            dll.send_mail_w_raw(std::ptr::null(), 0),
            Some(MapiResult::Failure.code())
        );
        assert_eq!(
            dll.send_mail_w_raw(0x1000 as *const MapiMessageW, 0),
            Some(MapiResult::Failure.code())
        );
        let garbage = MapiMessageW {
            nRecipCount: 3,
            lpRecips: 0xDEAD_0000 as *mut _,
            ..Default::default()
        };
        assert_eq!(
            dll.send_mail_w_raw(&garbage, 0),
            Some(MapiResult::InvalidRecips.code())
        );
    }
}

#[test]
fn no_app_running_and_none_to_start_fails() {
    let _env = TestEnv::new("down");
    let started = Instant::now();
    let message = CallerMessage {
        subject: Some("x".into()),
        ..Default::default()
    };
    assert_eq!(
        dll().send_mail_w(&message, MAPI_DIALOG),
        Some(MapiResult::Failure.code())
    );
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn starts_the_app_when_nothing_listens() {
    let env = TestEnv::new("start");
    let record_dir = tempfile::tempdir().unwrap();
    let record = record_dir.path().join("record.json");
    // The harness without arguments plays Nail.exe: it creates the pipe,
    // reads the request and the files, records them and answers.
    env.set(MAPI_APP_ENV, harness().as_os_str());
    env.set("NAIL_MAPI_HARNESS_RECORD", record.as_os_str());
    env.set("NAIL_MAPI_HARNESS_RESULT", "success".as_ref());
    let src = tempfile::tempdir().unwrap();
    let file = src.path().join("photo.jpg");
    std::fs::write(&file, "jpeg bytes").unwrap();
    let message = CallerMessage {
        subject: Some("Started".into()),
        files: vec![(file.to_str().unwrap().into(), None)],
        ..Default::default()
    };
    assert_eq!(dll().send_mail_w(&message, MAPI_DIALOG), Some(0));
    // The fake app writes its record right after answering.
    let deadline = Instant::now() + Duration::from_secs(10);
    let text = loop {
        if let Ok(text) = std::fs::read_to_string(&record)
            && !text.is_empty()
        {
            break text;
        }
        assert!(Instant::now() < deadline, "the fake app wrote no record");
        std::thread::sleep(Duration::from_millis(50));
    };
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["request"]["message"]["subject"], "Started");
    assert_eq!(json["files"][0]["name"], "photo.jpg");
    assert_eq!(json["files"][0]["content"], "jpeg bytes");
}

#[test]
fn logon_resolve_and_free_through_the_dll() {
    let dll = dll();
    assert_eq!(dll.logon(), Some((0, 1)));
    assert_eq!(dll.logoff(1), Some(0));
    let (code, name, address, freed) = dll.resolve_name("ada@x.example").unwrap();
    assert_eq!(code, 0);
    assert_eq!(name.as_deref(), Some("ada@x.example"));
    assert_eq!(address.as_deref(), Some("SMTP:ada@x.example"));
    assert_eq!(freed, 0);
    let (code, ..) = dll.resolve_name("Ada").unwrap();
    assert_eq!(code, MapiResult::UnknownRecipient.code());
    let mut foreign = 0u64;
    assert_eq!(
        // SAFETY: a pointer the DLL did not hand out.
        unsafe { dll.free_buffer((&raw mut foreign).cast()) },
        Some(MapiResult::Failure.code())
    );
}

#[test]
fn harness_commands() {
    let out = std::process::Command::new(harness())
        .args(["registry", r"C:\Program Files\Nail", "--os", "x64"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains(r"[HKEY_LOCAL_MACHINE\Software\Clients\Mail\Nail]"));
    assert!(text.contains(r#""DLLPath"="C:\\Program Files\\Nail\\mapi\\x64\\NailMapi.dll""#));
    let out = std::process::Command::new(harness())
        .args(["exports", "--dll"])
        .arg(dll_path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(
        String::from_utf8(out.stdout)
            .unwrap()
            .contains("MAPISendMailW\tok")
    );

    // `send` against a fake app on its own pipe (the e2e entry point).
    let env = TestEnv::new("cli");
    let app = fake_app(&env.pipe, MapiResult::Success);
    let out = std::process::Command::new(harness())
        .args(["send", "--dll"])
        .arg(dll_path())
        .args([
            "--to",
            "ada@x.example",
            "--subject",
            "From the CLI",
            "--no-ui",
        ])
        .env(MAPI_PIPE_ENV, &env.pipe)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let (request, _) = app.join().unwrap();
    assert_eq!(request.message.subject.as_deref(), Some("From the CLI"));
    assert_eq!(
        request
            .message
            .program
            .as_deref()
            .map(str::to_ascii_lowercase),
        Some("nail-mapi-harness.exe".into())
    );
}
