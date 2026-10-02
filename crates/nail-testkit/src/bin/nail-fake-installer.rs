//! A stand-in for Nail's installer in the desktop smoke test of the
//! updater (M4b WP115): the loopback update server offers this program as
//! the "new version", the app's updater downloads it, checks its
//! signature and runs it on "Restart to install" / quit — and instead of
//! installing anything it appends its command line (one JSON array per
//! run) to the file named by `NAIL_FAKE_INSTALLER_RECORD`, then exits.
//! Without the variable it does nothing.

use std::io::Write;

/// The variable naming the record file.
const RECORD_ENV: &str = "NAIL_FAKE_INSTALLER_RECORD";

fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// One JSON array of the arguments (without the program name).
fn record_line(args: &[String]) -> String {
    let items: Vec<String> = args.iter().map(|a| json_string(a)).collect();
    format!("[{}]\n", items.join(","))
}

fn main() {
    let Some(record) = std::env::var_os(RECORD_ENV) else {
        return;
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let written = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(record)
        .and_then(|mut file| file.write_all(record_line(&args).as_bytes()));
    if written.is_err() {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_the_arguments_as_json() {
        assert_eq!(
            record_line(&["/P".into(), "/UPDATE".into(), r#"C:\a "b""#.into()]),
            "[\"/P\",\"/UPDATE\",\"C:\\\\a \\\"b\\\"\"]\n"
        );
        assert_eq!(record_line(&[]), "[]\n");
        assert_eq!(json_string("\u{1}"), "\"\\u0001\"");
    }
}
