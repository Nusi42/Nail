//! Keeping one account's managed script on the server: compile against
//! the server's real extensions, check, upload, activate — and leave every
//! script of the user alone.
//!
//! * Nail's script is the one named [`ManageSieveConfig::effective_script_name`].
//!   A script with that name that does not start with
//!   [`crate::MANAGED_MARKER`] belongs to the user: Nail refuses to
//!   overwrite it ([`SieveError::NotManaged`]).
//! * When another script is active, Nail's script records its name
//!   ([`crate::PREVIOUS_MARKER`]) and, on servers with `include`, runs it
//!   (`include :personal`) so it keeps working after Nail's script becomes
//!   the active one. [`remove`] re-activates it.
//! * Only a script of the **user** counts as the previous one. An active
//!   script Nail wrote under another name (the script name was changed, or
//!   an older upload) is never included or re-activated: its own recorded
//!   previous script is taken over instead, and activating Nail's current
//!   script switches the old one off, so stale rules never run twice.
//!
//! [`ManageSieveConfig::effective_script_name`]: nail_api::sieve::ManageSieveConfig::effective_script_name

use nail_api::rules::Rule;
use nail_api::sieve::VacationSettings;

use crate::client::SieveSession;
use crate::compile::{CompileOptions, CompiledScript, MANAGED_MARKER, PREVIOUS_MARKER, compile};
use crate::error::{Result, SieveError};

/// What an upload did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadReport {
    /// The script as compiled for this server (and uploaded).
    pub compiled: CompiledScript,
    /// SHA-256 (hex) of `compiled.script`, as the store records it.
    pub script_hash: String,
    /// The server's Sieve extensions.
    pub extensions: Vec<String>,
    /// The other scripts on the server (left alone).
    pub other_scripts: Vec<String>,
    /// The user's script that was active before Nail's (recorded in
    /// Nail's script).
    pub previous_active: Option<String>,
    /// Whether the upload made Nail's script the active one (it was not).
    pub activated: bool,
    /// A script Nail wrote under another name that was active before and
    /// is now switched off (left on the server; the core removes it).
    pub replaced_managed: Option<String>,
    /// Warnings the server sent with `CHECKSCRIPT` / `PUTSCRIPT`.
    pub warnings: Vec<String>,
}

/// Whether `script` was written by Nail.
pub fn is_managed(script: &str) -> bool {
    script
        .lines()
        .find(|l| !l.trim().is_empty())
        .is_some_and(|l| l.trim_end() == MANAGED_MARKER)
}

/// The previously active script recorded in Nail's script.
pub fn recorded_previous(script: &str) -> Option<String> {
    script
        .lines()
        .find_map(|l| crate::writer::unquote(l.trim_end().strip_prefix(PREVIOUS_MARKER)?))
}

/// How many scripts written by Nail are followed through their recorded
/// previous script before giving up (a loop guard).
const MAX_PREVIOUS_HOPS: usize = 8;

/// The user's script `candidate` stands for: `candidate` itself when it
/// exists and is not Nail's; when it is a script Nail wrote (under another
/// name), the script *that* one recorded as previous, and so on. `None`
/// when nothing of the user's is left.
async fn users_previous(
    session: &mut SieveSession,
    scripts: &[crate::client::ScriptInfo],
    script_name: &str,
    mut candidate: Option<String>,
) -> Result<Option<String>> {
    let mut seen: Vec<String> = Vec::new();
    for _ in 0..MAX_PREVIOUS_HOPS {
        let Some(name) = candidate.take() else {
            return Ok(None);
        };
        if name == script_name || seen.contains(&name) || !scripts.iter().any(|s| s.name == name) {
            return Ok(None);
        }
        let content = session.get_script(&name).await?;
        if !is_managed(&content) {
            return Ok(Some(name));
        }
        candidate = recorded_previous(&content);
        seen.push(name);
    }
    Ok(None)
}

/// Compiles `rules` and `vacation` for the connected server and makes the
/// result the active script named `script_name`. `options.extensions`,
/// `options.max_redirects` and `options.previous_active` are taken from
/// the server.
pub async fn upload(
    session: &mut SieveSession,
    script_name: &str,
    rules: &[Rule],
    vacation: &VacationSettings,
    options: CompileOptions<'_>,
) -> Result<UploadReport> {
    let caps = session.capabilities().clone();
    let scripts = session.list_scripts().await?;
    let ours = scripts.iter().find(|s| s.name == script_name);
    let mut previous = None;
    if let Some(ours) = ours {
        let existing = session.get_script(&ours.name).await?;
        if !is_managed(&existing) {
            return Err(SieveError::NotManaged(script_name.to_owned()));
        }
        previous = recorded_previous(&existing);
    }
    let mut replaced_managed = None;
    if let Some(active) = scripts.iter().find(|s| s.active && s.name != script_name) {
        let content = session.get_script(&active.name).await?;
        if is_managed(&content) {
            // Nail's own script under an older name: never the user's
            // previous script; what it recorded is.
            replaced_managed = Some(active.name.clone());
            if let Some(theirs) = recorded_previous(&content) {
                previous = Some(theirs);
            }
        } else {
            previous = Some(active.name.clone());
        }
    }
    // A recorded script the user has deleted since is forgotten; one Nail
    // wrote is followed to the user's script it stood for.
    let previous = users_previous(session, &scripts, script_name, previous).await?;

    let options = CompileOptions {
        extensions: caps.extensions.clone(),
        max_redirects: caps.max_redirects,
        previous_active: previous.clone(),
        ..options
    };
    let compiled = compile(rules, vacation, &options);
    let mut warnings = Vec::new();
    if let Some(w) = session.check_script(&compiled.script).await? {
        warnings.extend(w);
    }
    warnings.extend(session.put_script(script_name, &compiled.script).await?);
    let activated = !ours.is_some_and(|s| s.active);
    if activated {
        session.set_active(script_name).await?;
    }
    Ok(UploadReport {
        script_hash: script_hash(&compiled.script),
        compiled,
        extensions: caps.extensions,
        other_scripts: scripts
            .iter()
            .filter(|s| s.name != script_name)
            .map(|s| s.name.clone())
            .collect(),
        previous_active: previous,
        activated,
        replaced_managed,
        warnings,
    })
}

/// Removes Nail's script (ManageSieve switched off for the account):
/// re-activates the user's script that was active before (if it still
/// exists; never a script Nail wrote) and deletes Nail's. Returns the
/// re-activated script. A script with Nail's name that Nail did not write
/// is left alone.
pub async fn remove(session: &mut SieveSession, script_name: &str) -> Result<Option<String>> {
    let scripts = session.list_scripts().await?;
    let Some(ours) = scripts.iter().find(|s| s.name == script_name) else {
        return Ok(None);
    };
    let existing = session.get_script(script_name).await?;
    if !is_managed(&existing) {
        return Err(SieveError::NotManaged(script_name.to_owned()));
    }
    let previous = if ours.active {
        users_previous(session, &scripts, script_name, recorded_previous(&existing)).await?
    } else {
        None
    };
    if ours.active {
        session
            .set_active(previous.as_deref().unwrap_or_default())
            .await?;
    }
    session.delete_script(script_name).await?;
    Ok(previous)
}

/// SHA-256 (hex) of a script, as `nail-store` records it
/// (`hash_sieve_script`).
pub fn script_hash(script: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(script.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers() {
        let script = format!("{MANAGED_MARKER}\n{PREVIOUS_MARKER}\"my \\\"rules\\\"\"\nkeep;\n");
        assert!(is_managed(&script));
        assert_eq!(recorded_previous(&script).as_deref(), Some("my \"rules\""));
        assert!(is_managed(&format!("\r\n{MANAGED_MARKER}\r\nkeep;")));
        assert!(!is_managed("# my own\nkeep;"));
        assert!(!is_managed(""));
        assert_eq!(recorded_previous("keep;"), None);
        // SHA-256 of the empty string (FIPS 180-2 test vector).
        assert_eq!(
            script_hash(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(script_hash("keep;\n").len(), 64);
    }
}
