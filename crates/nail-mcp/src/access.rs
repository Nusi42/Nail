//! Grants applied to data: which accounts, folders and scopes a client may
//! read. A new client has an empty grant and sees nothing (decided in
//! design.md: no access until granted).

use std::collections::BTreeSet;

use nail_api::mcp::{McpClient, McpGrant, McpScope};
use nail_api::{AccountId, FolderId, MessageSummary, VirtualAccountConfig};

/// Why a call was refused (written to the audit log, returned to the
/// agent). English: agents and developer tools read it, not the UI.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Denial {
    /// A scope is missing.
    #[error("scope `{}` not granted", scope_name(*.0))]
    Scope(McpScope),
    /// The account is not granted.
    #[error("account `{0}` not granted")]
    Account(AccountId),
    /// The folder is excluded by the grant.
    #[error("folder `{0}` excluded or not in a granted account")]
    Folder(FolderId),
    /// The resource spans every account (rules), but the grant does not.
    #[error("needs a grant for all accounts")]
    AllAccounts,
}

/// The wire name of a scope (`raw-source`, …).
pub fn scope_name(scope: McpScope) -> String {
    match serde_json::to_value(scope) {
        Ok(serde_json::Value::String(s)) => s,
        _ => format!("{scope:?}"),
    }
}

/// A client's grant, resolved against the current virtual accounts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Access {
    scopes: BTreeSet<McpScope>,
    /// `None` = every account.
    accounts: Option<BTreeSet<AccountId>>,
    excluded: BTreeSet<FolderId>,
}

impl Access {
    /// Resolves `grant`: a virtual account in `grant.accounts` stands for
    /// its members, the built-in `all` for every account. An empty grant
    /// (no scope, or no account) allows nothing at all.
    pub fn resolve(grant: &McpGrant, virtuals: &[VirtualAccountConfig]) -> Self {
        if grant.is_empty() {
            return Self::none();
        }
        let everything =
            grant.all_accounts || grant.accounts.iter().any(|a| a.as_str() == AccountId::ALL);
        let accounts = (!everything).then(|| {
            let mut set: BTreeSet<AccountId> = grant.accounts.iter().cloned().collect();
            for va in virtuals {
                if grant.accounts.contains(&va.id) {
                    set.extend(va.members.iter().cloned());
                }
            }
            set
        });
        Self {
            scopes: grant.scopes.iter().copied().collect(),
            accounts,
            excluded: grant.excluded_folders.iter().cloned().collect(),
        }
    }

    /// No access at all.
    pub fn none() -> Self {
        Self {
            scopes: BTreeSet::new(),
            accounts: Some(BTreeSet::new()),
            excluded: BTreeSet::new(),
        }
    }

    /// The client's grant, resolved.
    pub fn of(client: &McpClient, virtuals: &[VirtualAccountConfig]) -> Self {
        Self::resolve(&client.grant, virtuals)
    }

    /// Whether `scope` is granted.
    pub fn has(&self, scope: McpScope) -> bool {
        self.scopes.contains(&scope)
    }

    /// Fails unless `scope` is granted.
    pub fn require(&self, scope: McpScope) -> Result<(), Denial> {
        if self.has(scope) {
            Ok(())
        } else {
            Err(Denial::Scope(scope))
        }
    }

    /// Whether the account (real or virtual) may be read.
    pub fn allows_account(&self, account: &AccountId) -> bool {
        self.accounts
            .as_ref()
            .is_none_or(|set| set.contains(account))
    }

    /// Whether every account is granted (so unfiltered totals are exact).
    pub fn all_accounts(&self) -> bool {
        self.accounts.is_none()
    }

    /// Whether the folder is not excluded.
    pub fn allows_folder(&self, folder: &FolderId) -> bool {
        !self.excluded.contains(folder)
    }

    /// Whether folders are excluded (so unfiltered totals are exact).
    pub fn excludes_folders(&self) -> bool {
        !self.excluded.is_empty()
    }

    /// Fails unless `read` is granted and the account may be read.
    pub fn require_account(&self, account: &AccountId) -> Result<(), Denial> {
        self.require(McpScope::Read)?;
        if self.allows_account(account) {
            Ok(())
        } else {
            Err(Denial::Account(account.clone()))
        }
    }

    /// Fails unless `read` is granted and the message's account and folder
    /// may be read.
    pub fn require_message(&self, message: &MessageSummary) -> Result<(), Denial> {
        self.require_account(&message.account_id)?;
        if self.allows_folder(&message.folder_id) {
            Ok(())
        } else {
            Err(Denial::Folder(message.folder_id.clone()))
        }
    }

    /// Whether the message may be read (see [`Self::require_message`]).
    pub fn allows_message(&self, message: &MessageSummary) -> bool {
        self.require_message(message).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::Flags;

    fn summary(account: &str, folder: &str) -> MessageSummary {
        MessageSummary {
            id: "m1".into(),
            folder_id: folder.into(),
            account_id: account.into(),
            subject: String::new(),
            from: None,
            to: vec![],
            date: String::new(),
            flags: Flags::default(),
            has_attachments: false,
            preview: String::new(),
            thread_id: None,
            tags: vec![],
            duplicates: vec![],
            headers_only: false,
        }
    }

    fn virtual_account(id: &str, members: &[&str]) -> VirtualAccountConfig {
        serde_json::from_value(serde_json::json!({
            "id": id, "name": id, "members": members,
        }))
        .unwrap()
    }

    #[test]
    fn empty_grants_allow_nothing() {
        let access = Access::resolve(&McpGrant::default(), &[]);
        assert_eq!(access, Access::none());
        assert_eq!(
            access.require_message(&summary("work", "inbox")),
            Err(Denial::Scope(McpScope::Read))
        );
        // Scopes without any account are still nothing.
        let scopes_only = McpGrant {
            scopes: vec![McpScope::Read, McpScope::RawSource],
            ..McpGrant::default()
        };
        let access = Access::resolve(&scopes_only, &[]);
        assert!(!access.has(McpScope::Read));
        assert!(!access.allows_account(&"work".into()));
    }

    #[test]
    fn accounts_folders_and_scopes_are_checked() {
        let grant = McpGrant {
            scopes: vec![McpScope::Read],
            accounts: vec!["work".into()],
            excluded_folders: vec!["work/hr".into()],
            ..McpGrant::default()
        };
        let access = Access::resolve(&grant, &[]);
        assert!(access.allows_message(&summary("work", "work/inbox")));
        assert_eq!(
            access.require_message(&summary("work", "work/hr")),
            Err(Denial::Folder("work/hr".into()))
        );
        assert_eq!(
            Denial::AllAccounts.to_string(),
            "needs a grant for all accounts"
        );
        assert_eq!(
            access.require_message(&summary("home", "home/inbox")),
            Err(Denial::Account("home".into()))
        );
        assert_eq!(
            access.require(McpScope::RawSource),
            Err(Denial::Scope(McpScope::RawSource))
        );
        assert_eq!(
            Denial::Scope(McpScope::RawSource).to_string(),
            "scope `raw-source` not granted"
        );
        assert!(!access.all_accounts() && access.excludes_folders());
    }

    #[test]
    fn virtual_accounts_grant_their_members() {
        let grant = McpGrant {
            scopes: vec![McpScope::Read],
            accounts: vec!["both".into()],
            ..McpGrant::default()
        };
        let access = Access::resolve(&grant, &[virtual_account("both", &["work", "home"])]);
        assert!(access.allows_account(&"both".into()));
        assert!(access.allows_account(&"work".into()));
        assert!(access.allows_account(&"home".into()));
        assert!(!access.allows_account(&"other".into()));

        let all = McpGrant {
            scopes: vec![McpScope::Read],
            accounts: vec![AccountId::all()],
            ..McpGrant::default()
        };
        assert!(Access::resolve(&all, &[]).all_accounts());
        let all = McpGrant {
            scopes: vec![McpScope::Read],
            all_accounts: true,
            ..McpGrant::default()
        };
        assert!(Access::resolve(&all, &[]).allows_account(&"anything".into()));
    }
}
