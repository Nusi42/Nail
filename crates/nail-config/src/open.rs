//! "Open file at line": the command that opens a configuration file in the
//! user's editor (the inspector's jump to the layout node or theme rule).
//!
//! The editor comes from `NAIL_EDITOR`, `VISUAL` or `EDITOR` (in that
//! order). A value containing `{file}` is a template (`{line}` and
//! `{column}` are replaced too); otherwise well-known editors get their
//! "go to line" arguments, and any other program just gets the path.
//! Without a configured editor the system's text editor opens the file.

use std::path::Path;
use std::process::Command;

/// Environment variables naming the editor, in order of preference.
pub const EDITOR_VARS: [&str; 3] = ["NAIL_EDITOR", "VISUAL", "EDITOR"];

/// The editor configured in the environment, if any.
pub fn configured_editor() -> Option<String> {
    EDITOR_VARS
        .iter()
        .filter_map(|var| std::env::var(var).ok())
        .map(|v| v.trim().to_owned())
        .find(|v| !v.is_empty())
}

/// Splits a command line into words (double or single quotes group).
fn words(spec: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut any = false;
    for c in spec.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                any = true;
            }
            None if c.is_whitespace() => {
                if any || !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                    any = false;
                }
            }
            // Inside quotes, or a plain character.
            Some(_) | None => current.push(c),
        }
    }
    if any || !current.is_empty() {
        out.push(current);
    }
    out
}

/// The command opening `path` at `line`:`column` (1-based) with `editor`
/// (see the module docs); `None` = the system's text editor.
pub fn editor_command(path: &Path, line: u32, column: u32, editor: Option<&str>) -> Command {
    let file = path.display().to_string();
    let Some(spec) = editor.map(str::trim).filter(|s| !s.is_empty()) else {
        return system_editor(&file);
    };
    let mut parts = words(spec);
    if parts.is_empty() {
        return system_editor(&file);
    }
    let program = parts.remove(0);
    let mut command = Command::new(&program);
    if spec.contains("{file}") {
        for part in parts {
            command.arg(
                part.replace("{file}", &file)
                    .replace("{line}", &line.to_string())
                    .replace("{column}", &column.to_string()),
            );
        }
        return command;
    }
    command.args(&parts);
    let name = Path::new(&program)
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match name.as_str() {
        "code" | "code-insiders" | "codium" | "cursor" | "windsurf" => {
            command.arg("-g").arg(format!("{file}:{line}:{column}"));
        }
        "notepad++" => {
            command
                .arg(format!("-n{line}"))
                .arg(format!("-c{column}"))
                .arg(&file);
        }
        "vim" | "nvim" | "vi" | "gvim" | "nano" | "micro" | "kak" => {
            command.arg(format!("+{line}")).arg(&file);
        }
        "emacs" | "emacsclient" => {
            command.arg(format!("+{line}:{column}")).arg(&file);
        }
        "subl" | "sublime_text" | "zed" | "hx" | "helix" => {
            command.arg(format!("{file}:{line}:{column}"));
        }
        _ => {
            command.arg(&file);
        }
    }
    command
}

fn system_editor(file: &str) -> Command {
    if cfg!(windows) {
        let mut command = Command::new("notepad.exe");
        command.arg(file);
        command
    } else if cfg!(target_os = "macos") {
        let mut command = Command::new("open");
        command.arg("-t").arg(file);
        command
    } else {
        let mut command = Command::new("xdg-open");
        command.arg(file);
        command
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(command: &Command) -> (String, Vec<String>) {
        (
            command.get_program().to_string_lossy().into_owned(),
            command
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect(),
        )
    }

    #[test]
    fn known_editors_jump_to_the_line() {
        let path = Path::new("cfg/layout.kdl");
        let file = path.display().to_string();
        assert_eq!(
            args(&editor_command(path, 12, 5, Some("code --wait"))),
            (
                "code".into(),
                vec!["--wait".into(), "-g".into(), format!("{file}:12:5")]
            )
        );
        assert_eq!(
            args(&editor_command(
                path,
                3,
                1,
                Some("\"C:\\Program Files\\Notepad++\\notepad++.exe\"")
            )),
            (
                "C:\\Program Files\\Notepad++\\notepad++.exe".into(),
                vec!["-n3".into(), "-c1".into(), file.clone()]
            )
        );
        assert_eq!(
            args(&editor_command(path, 7, 2, Some("nvim"))),
            ("nvim".into(), vec!["+7".into(), file.clone()])
        );
        assert_eq!(
            args(&editor_command(path, 7, 2, Some("emacs"))),
            ("emacs".into(), vec!["+7:2".into(), file.clone()])
        );
        assert_eq!(
            args(&editor_command(path, 7, 2, Some("myeditor"))),
            ("myeditor".into(), vec![file.clone()])
        );
    }

    #[test]
    fn templates_are_filled_in() {
        let path = Path::new("theme.kdl");
        assert_eq!(
            args(&editor_command(
                path,
                4,
                9,
                Some("ed '--at={line},{column}' {file}")
            )),
            ("ed".into(), vec!["--at=4,9".into(), "theme.kdl".into()])
        );
    }

    #[test]
    fn without_an_editor_the_system_one_opens_the_file() {
        let (program, args) = args(&editor_command(Path::new("a.kdl"), 1, 1, None));
        assert!(!program.is_empty(), "{program:?}");
        assert_eq!(args.last().map(String::as_str), Some("a.kdl"));
        let (_, blank) = self::args(&editor_command(Path::new("a.kdl"), 1, 1, Some("  ")));
        assert_eq!(blank.last().map(String::as_str), Some("a.kdl"));
    }

    #[test]
    fn words_split_like_a_shell() {
        assert_eq!(words("a 'b c' \"d\" ''"), ["a", "b c", "d", ""]);
        assert_eq!(words("   ").len(), 0);
    }
}
