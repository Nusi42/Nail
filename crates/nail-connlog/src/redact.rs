//! Text redaction shared by every protocol rule, the application log and the
//! diagnostics bundle (ADR 0190; moved here from `nail-portability`).
//!
//! * [`redact_secrets`]: values of secret-looking keys, `Bearer` / `Basic`
//!   credentials, `keychain:` references and PEM private keys;
//! * [`replace_addresses`]: the one mail-address scanner (masked in logs by
//!   [`mask_address`], replaced by stable placeholders in the bundle);
//! * [`redact_subjects`]: `Subject:` values and display names;
//! * [`redact_user_paths`]: the user's name in home folder paths;
//! * [`Secrets`]: the exact secrets a connection registered;
//! * [`scrub`]: what every logged line goes through last.

pub use nail_api::connlog::REDACTED;

/// Secrets shorter than this are not searched for as substrings (they
/// would match ordinary text); the protocol rules still remove them where
/// they are sent.
pub const MIN_SECRET_LEN: usize = 4;

const SECRET_KEYS: [&str; 16] = [
    "password",
    "passwd",
    "pwd",
    "secret",
    "token",
    "access_token",
    "refresh_token",
    "id_token",
    "client_secret",
    "code_verifier",
    "assertion",
    "api_key",
    "apikey",
    "authorization",
    "cookie",
    "pin",
];

/// Removes values of secret-looking keys (`password=…`, `"token": "…"`,
/// `Authorization: Bearer …`), `keychain:` references and PEM private
/// keys.
pub fn redact_secrets(text: &str) -> String {
    let mut lines = Vec::new();
    let mut in_pem = false;
    for line in text.split_inclusive('\n') {
        if line.contains("-----BEGIN") && line.contains("PRIVATE KEY") {
            in_pem = true;
            lines.push(format!("{REDACTED}{}", line_end(line)));
            continue;
        }
        if in_pem {
            if line.contains("-----END") {
                in_pem = false;
            }
            continue;
        }
        lines.push(redact_secret_line(line));
    }
    lines.concat()
}

fn line_end(line: &str) -> &str {
    if line.ends_with("\r\n") {
        "\r\n"
    } else if line.ends_with('\n') {
        "\n"
    } else {
        ""
    }
}

fn redact_secret_line(line: &str) -> String {
    let mut out = line.to_owned();
    // `keychain:` references.
    let mut search = 0;
    while let Some(at) = out[search..].find("keychain:") {
        let start = search + at + "keychain:".len();
        let end = out[start..]
            .find(|c: char| c.is_whitespace() || "\"',;)>]}".contains(c))
            .map_or(out.len(), |e| start + e);
        out.replace_range(start..end, REDACTED);
        search = start + REDACTED.len();
    }
    // Bearer / Basic credentials.
    for scheme in ["Bearer ", "bearer ", "Basic ", "basic "] {
        let mut search = 0;
        while let Some(at) = out[search..].find(scheme) {
            let start = search + at + scheme.len();
            let end = out[start..]
                .find(|c: char| c.is_whitespace() || "\"',;".contains(c))
                .map_or(out.len(), |e| start + e);
            if end > start {
                out.replace_range(start..end, REDACTED);
            }
            search = start + REDACTED.len().min(out.len() - start);
        }
    }
    // key=value, key: value, "key": "value".
    let lower = out.to_ascii_lowercase();
    let mut replacements: Vec<(usize, usize)> = Vec::new();
    for key in SECRET_KEYS {
        let mut search = 0;
        while let Some(at) = lower[search..].find(key) {
            let key_start = search + at;
            let key_end = key_start + key.len();
            search = key_end;
            // A whole word: not part of a longer identifier.
            let before = lower[..key_start].chars().next_back();
            let after = lower[key_end..].chars().next();
            if before.is_some_and(|c| c.is_ascii_alphanumeric())
                || after.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                continue;
            }
            let rest = &out[key_end..];
            let trimmed = rest.trim_start_matches(['"', '\'']).trim_start();
            let Some(sep) = trimmed.chars().next().filter(|c| *c == '=' || *c == ':') else {
                continue;
            };
            let value_start = key_end + (rest.len() - trimmed.len()) + sep.len_utf8();
            let value_text = &out[value_start..];
            let skip_ws = value_text.len() - value_text.trim_start().len();
            let value_start = value_start + skip_ws;
            let value_text = &out[value_start..];
            let (start, end) = if let Some(quote) = value_text
                .chars()
                .next()
                .filter(|c| *c == '"' || *c == '\'')
            {
                let close = value_text[1..]
                    .find(quote)
                    .map_or(value_text.len(), |e| e + 1);
                (value_start + 1, value_start + close)
            } else {
                let end = value_text
                    .find(|c: char| c.is_whitespace() || ",;}&".contains(c))
                    .unwrap_or(value_text.len());
                (value_start, value_start + end)
            };
            // Already redacted, or nothing.
            if end > start && &out[start..end] != REDACTED && !out[start..end].starts_with("Bearer")
            {
                replacements.push((start, end));
            }
        }
    }
    replacements.sort_unstable();
    replacements.dedup();
    let mut result = String::with_capacity(out.len());
    let mut copied = 0;
    for (start, end) in replacements {
        if start < copied {
            continue;
        }
        result.push_str(&out[copied..start]);
        result.push_str(REDACTED);
        copied = end;
    }
    result.push_str(&out[copied..]);
    result
}

/// Replaces every mail address in `text` by `replace(address)` (the address
/// as written). The one address scanner of the workspace.
pub fn replace_addresses(text: &str, mut replace: impl FnMut(&str) -> String) -> String {
    if !text.contains('@') {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let is_local = |c: char| c.is_alphanumeric() || "._%+-'".contains(c);
    let is_domain = |c: char| c.is_alphanumeric() || ".-".contains(c);
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut copied = 0;
    while i < chars.len() {
        if chars[i] == '@' {
            let mut start = i;
            while start > copied && is_local(chars[start - 1]) {
                start -= 1;
            }
            let mut end = i + 1;
            while end < chars.len() && is_domain(chars[end]) {
                end += 1;
            }
            // Trailing dots are punctuation.
            while end > i + 1 && chars[end - 1] == '.' {
                end -= 1;
            }
            let domain: String = chars[i + 1..end].iter().collect();
            if start < i && domain.contains('.') && !domain.starts_with('.') {
                let address: String = chars[start..end].iter().collect();
                out.extend(&chars[copied..start]);
                out.push_str(&replace(&address));
                copied = end;
                i = end;
                continue;
            }
        }
        i += 1;
    }
    out.extend(&chars[copied..]);
    out
}

/// A masked address: the local part's first character, `***`, the domain
/// (`sam@example.net` → `s***@example.net`).
pub fn mask_address(address: &str) -> String {
    match address.rsplit_once('@') {
        Some((local, domain)) => format!("{}***@{domain}", first_char(local)),
        None => mask_user(address),
    }
}

/// A masked user name: an address as [`mask_address`], anything else its
/// first character and `***`.
pub fn mask_user(name: &str) -> String {
    if name.contains('@') {
        let masked = replace_addresses(name, mask_address);
        if masked != name {
            return masked;
        }
    }
    format!("{}***", first_char(name))
}

fn first_char(text: &str) -> String {
    text.chars()
        .find(|c| !c.is_whitespace() && *c != '"')
        .map(String::from)
        .unwrap_or_default()
}

/// [`replace_addresses`] with [`mask_address`].
pub fn mask_addresses(text: &str) -> String {
    replace_addresses(text, mask_address)
}

/// Removes subjects (`Subject: …` lines, `subject="…"` fields) and display
/// names before addresses (`Ada Lovelace <…>`).
pub fn redact_subjects(text: &str) -> String {
    text.split_inclusive('\n')
        .map(|line| {
            let lower = line.to_ascii_lowercase();
            if let Some(at) = lower.find("subject") {
                let after = &line[at + "subject".len()..];
                let trimmed = after.trim_start_matches(['"', '\'']).trim_start();
                if trimmed.starts_with(['=', ':']) {
                    let value_start = at + "subject".len() + (after.len() - trimmed.len()) + 1;
                    let value = line[value_start..].trim_start();
                    let value_start = line.len() - value.len();
                    // A quoted value ends at its quote, else at the line end.
                    if let Some(quote) = value.chars().next().filter(|c| *c == '"' || *c == '\'')
                        && let Some(close) = value[1..].find(quote)
                    {
                        let end = value_start + 1 + close;
                        return format!(
                            "{}{REDACTED}{}",
                            &line[..=value_start],
                            redact_display_names(&line[end..])
                        );
                    }
                    return format!(
                        "{} {REDACTED}{}",
                        line[..value_start].trim_end(),
                        line_end(line)
                    );
                }
            }
            redact_display_names(line)
        })
        .collect()
}

fn redact_display_names(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(lt) = rest.find('<') {
        let Some(gt) = rest[lt..].find('>') else {
            break;
        };
        let inside = &rest[lt + 1..lt + gt];
        if inside.contains('@') {
            let before = &rest[..lt];
            // The name runs back to a separator.
            let name_start = before.rfind([',', ':', '(', '[', '=']).map_or(0, |i| i + 1);
            let name = before[name_start..].trim();
            if !name.is_empty() {
                out.push_str(&before[..name_start]);
                out.push(' ');
                out.push_str(REDACTED);
                out.push(' ');
                out.push_str(&rest[lt..=(lt + gt)]);
                rest = &rest[lt + gt + 1..];
                continue;
            }
        }
        out.push_str(&rest[..=(lt + gt)]);
        rest = &rest[lt + gt + 1..];
    }
    out.push_str(rest);
    out
}

/// Replaces the user's name inside home folder paths
/// (`C:\Users\sam\…`, `/home/sam/…`, `C:/Users/sam`).
pub fn redact_user_paths(text: &str, user: &str) -> String {
    let mut out = text.to_owned();
    for prefix in ["Users\\", "Users/", "users\\", "users/", "home/", "HOME/"] {
        let needle = format!("{prefix}{user}");
        let mut search = 0;
        while let Some(at) = out[search..]
            .to_ascii_lowercase()
            .find(&needle.to_ascii_lowercase())
        {
            let start = search + at + prefix.len();
            let end = start + user.len();
            let next = out[end..].chars().next();
            if next.is_none_or(|c| c == '\\' || c == '/' || !c.is_alphanumeric()) {
                out.replace_range(start..end, "<user>");
                search = start + "<user>".len();
            } else {
                search = end;
            }
        }
    }
    out
}

/// The exact secrets a connection (or an account's tap) registered: the
/// password, app password or token it signed in with. Every occurrence is
/// removed from logged text.
#[derive(Debug, Clone, Default)]
pub struct Secrets {
    values: Vec<String>,
}

/// Secrets remembered at most (an account signs in with few; tokens are
/// refreshed now and then).
const MAX_SECRETS: usize = 16;

impl Secrets {
    /// Remembers `secret` (ignored when shorter than [`MIN_SECRET_LEN`]).
    pub fn add(&mut self, secret: &str) {
        if secret.len() < MIN_SECRET_LEN || self.values.iter().any(|v| v == secret) {
            return;
        }
        if self.values.len() >= MAX_SECRETS {
            self.values.remove(0);
        }
        self.values.push(secret.to_owned());
        // Longest first: a secret containing another one goes as a whole.
        self.values.sort_by_key(|v| std::cmp::Reverse(v.len()));
    }

    /// Whether nothing is registered.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// `text` with every registered secret replaced by [`REDACTED`].
    pub fn remove_from(&self, text: &str) -> String {
        let mut out = text.to_owned();
        for secret in &self.values {
            if out.contains(secret.as_str()) {
                out = out.replace(secret.as_str(), REDACTED);
            }
        }
        out
    }
}

/// The last step for every logged text: registered secrets, secret-looking
/// values, masked addresses.
pub fn scrub(text: &str, secrets: &Secrets) -> String {
    mask_addresses(&redact_secrets(&secrets.remove_from(text)))
}

/// `text` cut to at most `max` bytes (at a character boundary), ending with
/// how many bytes were left out.
pub fn cut(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… (+{} bytes)", &text[..end], text.len() - end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn secrets_are_always_removed() {
        let text = "connect user=ada password=hunter2 ok\n\
                    {\"refresh_token\": \"1//abc\", \"name\": \"x\"}\n\
                    Authorization: Bearer eyJhbGciOi\n\
                    account \"work\" { password \"keychain:nail/work/password\" }\n\
                    passwordless=true pin: 1234\n\
                    -----BEGIN PRIVATE KEY-----\nMIIEv\nabc\n-----END PRIVATE KEY-----\nafter\n";
        let out = redact_secrets(text);
        assert!(!out.contains("hunter2"), "{out}");
        assert!(!out.contains("1//abc"), "{out}");
        assert!(!out.contains("eyJhbGciOi"), "{out}");
        assert!(!out.contains("nail/work/password"), "{out}");
        assert!(!out.contains("1234"), "{out}");
        assert!(!out.contains("MIIEv"), "{out}");
        assert!(out.contains("user=ada"));
        assert!(out.contains("passwordless=true"), "not a secret key: {out}");
        assert!(out.contains("\"name\": \"x\""));
        assert!(out.ends_with("after\n"));
    }

    #[test]
    fn addresses_are_masked() {
        assert_eq!(mask_address("sam@example.net"), "s***@example.net");
        assert_eq!(mask_user("sam"), "s***");
        assert_eq!(mask_user("\"sam@example.net\""), "\"s***@example.net\"");
        assert_eq!(
            mask_addresses("MAIL FROM:<Ada@Example.org> to bob@example.org."),
            "MAIL FROM:<A***@Example.org> to b***@example.org."
        );
        assert_eq!(mask_addresses("no address @ all"), "no address @ all");
        let mut seen = Vec::new();
        let out = replace_addresses("a@b.org and c@d.org", |a| {
            seen.push(a.to_owned());
            "x".into()
        });
        assert_eq!(out, "x and x");
        assert_eq!(seen, ["a@b.org", "c@d.org"]);
    }

    #[test]
    fn subjects_names_and_paths() {
        let out = redact_subjects("Subject: Secret plans\nfrom Ada Lovelace <ada@example.org>\n");
        assert!(!out.contains("Secret plans"), "{out}");
        assert!(!out.contains("Ada Lovelace"), "{out}");
        assert_eq!(
            redact_user_paths("C:\\Users\\sam\\AppData and C:\\Users\\samother", "sam"),
            "C:\\Users\\<user>\\AppData and C:\\Users\\samother"
        );
    }

    #[test]
    fn registered_secrets_and_scrub() {
        let mut secrets = Secrets::default();
        secrets.add("abc");
        assert!(secrets.is_empty(), "too short to search for");
        secrets.add("s3cret-pass");
        secrets.add("s3cret");
        assert_eq!(
            scrub(
                "NO login s3cret-pass for sam@example.net (s3cret)",
                &secrets
            ),
            "NO login <redacted> for s***@example.net (<redacted>)"
        );
        for n in 0..20 {
            secrets.add(&format!("token-{n:04}"));
        }
        assert!(secrets.remove_from("token-0019").contains(REDACTED));
    }

    #[test]
    fn cutting_keeps_characters_whole() {
        assert_eq!(cut("short", 10), "short");
        assert_eq!(cut("ääää", 3), "ä… (+6 bytes)");
    }
}
