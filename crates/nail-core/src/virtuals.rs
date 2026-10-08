//! Virtual accounts: the built-in "All accounts" and the user's own
//! (`accounts.kdl`), shown with folders like real accounts (ADR 0012).
//!
//! A virtual account's folders are computed, never stored:
//!
//! * with `merge_roles`, one folder per role (Inbox, Sent, Drafts, Archive,
//!   Junk, Trash) merging the members' folders with that role;
//! * one folder per hand-made mapping (`map "<name>" { folder … }`);
//! * every other member folder below a per-account node, so nothing is
//!   hidden.
//!
//! Their ids are `<virtual account>/<key>` (real folder ids never contain a
//! `/`, and account ids cannot), so a virtual folder id names its virtual
//! account. Message lists over a virtual folder collapse copies of one mail
//! (same Message-ID) into one row with the others in `duplicates`.

use std::collections::{HashMap, HashSet};

use nail_api::{
    Account, AccountId, AccountKind, AccountsConfig, Folder, FolderId, FolderRole,
    VirtualAccountConfig,
};

/// Roles merged across the members of a virtual account, in display order.
pub(crate) const MERGED_ROLES: [FolderRole; 6] = [
    FolderRole::Inbox,
    FolderRole::Sent,
    FolderRole::Drafts,
    FolderRole::Archive,
    FolderRole::Junk,
    FolderRole::Trash,
];

/// A folder of a virtual account and the real folders behind it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VirtualFolder {
    /// The folder as listed.
    pub folder: Folder,
    /// The real folders it shows.
    pub sources: Vec<FolderId>,
}

/// Whether a folder id names a virtual folder.
pub(crate) fn is_virtual_folder(id: &FolderId) -> bool {
    id.as_str().contains('/')
}

/// The virtual account a virtual folder id belongs to.
pub(crate) fn virtual_account_of(id: &FolderId) -> Option<AccountId> {
    id.as_str()
        .split_once('/')
        .map(|(account, _)| AccountId::new(account))
}

/// The accounts that take part in "All accounts": every enabled account
/// from `accounts.kdl` plus local accounts the store has (e.g. demo data).
pub(crate) fn all_members(config: &AccountsConfig, stored: &[Account]) -> Vec<AccountId> {
    let mut members: Vec<AccountId> = config
        .accounts
        .iter()
        .filter(|a| a.enabled)
        .map(|a| a.id.clone())
        .collect();
    for account in stored {
        if extra_account(config, account) && !members.contains(&account.id) {
            members.push(account.id.clone());
        }
    }
    members
}

/// A stored account shown although it is not in `accounts.kdl`: local
/// accounts (demo data) only. IMAP / POP3 rows without configuration are
/// leftovers of a hand-removed account and stay hidden (their mail is kept
/// until the account is removed through the UI).
pub(crate) fn extra_account(config: &AccountsConfig, account: &Account) -> bool {
    config.account(&account.id).is_none()
        && config.virtual_account(&account.id).is_none()
        && account.kind == AccountKind::Local
}

/// The built-in "All accounts" virtual account.
pub(crate) fn all_accounts(config: &AccountsConfig, stored: &[Account]) -> VirtualAccountConfig {
    VirtualAccountConfig {
        id: AccountId::all(),
        name: String::new(),
        members: all_members(config, stored),
        merge_roles: true,
        folder_mappings: Vec::new(),
        default_identity: None,
        identity_rules: Vec::new(),
    }
}

/// The configuration of a virtual account id (`all` included).
pub(crate) fn virtual_config(
    config: &AccountsConfig,
    stored: &[Account],
    id: &AccountId,
) -> Option<VirtualAccountConfig> {
    if id.as_str() == AccountId::ALL {
        Some(all_accounts(config, stored))
    } else {
        config.virtual_account(id).cloned()
    }
}

/// The folders of `va` built from its members' folders (with counts).
/// `names` gives the display names of member accounts.
pub(crate) fn virtual_folders(
    va: &VirtualAccountConfig,
    member_folders: &[Folder],
    names: &HashMap<AccountId, String>,
) -> Vec<VirtualFolder> {
    let mut out = Vec::new();
    let mut used: HashSet<FolderId> = HashSet::new();
    let by_id: HashMap<&FolderId, &Folder> = member_folders.iter().map(|f| (&f.id, f)).collect();
    let make = |id: String,
                name: String,
                path: String,
                role: FolderRole,
                parent: Option<FolderId>,
                sources: Vec<FolderId>| {
        let (unread, total) = sources
            .iter()
            .filter_map(|s| by_id.get(s))
            .fold((0u32, 0u32), |(u, t), f| {
                (u.saturating_add(f.unread), t.saturating_add(f.total))
            });
        VirtualFolder {
            folder: Folder {
                id: FolderId::new(id),
                account_id: va.id.clone(),
                parent_id: parent,
                name,
                path,
                role,
                unread,
                total,
            },
            sources,
        }
    };
    let members: Vec<&Folder> = member_folders
        .iter()
        .filter(|f| va.members.contains(&f.account_id))
        .collect();
    if va.merge_roles {
        for role in MERGED_ROLES {
            // Members in configuration order, then by path.
            let mut sources: Vec<&Folder> =
                members.iter().copied().filter(|f| f.role == role).collect();
            sources.sort_by_key(|f| {
                (
                    va.members.iter().position(|m| *m == f.account_id),
                    f.path.clone(),
                )
            });
            let Some(first) = sources.first() else {
                continue;
            };
            let name = first.name.clone();
            used.extend(sources.iter().map(|f| f.id.clone()));
            out.push(make(
                format!("{}/{}", va.id, role.as_str()),
                name.clone(),
                name,
                role,
                None,
                sources.iter().map(|f| f.id.clone()).collect(),
            ));
        }
    }
    for (index, mapping) in va.folder_mappings.iter().enumerate() {
        let sources: Vec<FolderId> = mapping
            .sources
            .iter()
            .filter_map(|s| {
                members
                    .iter()
                    .find(|f| f.account_id == s.account_id && f.path == s.path)
                    .map(|f| f.id.clone())
            })
            .collect();
        used.extend(sources.iter().cloned());
        out.push(make(
            format!("{}/map-{index}", va.id),
            mapping.name.clone(),
            mapping.name.clone(),
            FolderRole::Other,
            None,
            sources,
        ));
    }
    for account in &va.members {
        let rest: Vec<&Folder> = members
            .iter()
            .copied()
            .filter(|f| &f.account_id == account && !used.contains(&f.id))
            .collect();
        if rest.is_empty() {
            continue;
        }
        let account_name = names
            .get(account)
            .filter(|n| !n.is_empty())
            .cloned()
            .unwrap_or_else(|| account.to_string());
        let parent_id = format!("{}/account-{account}", va.id);
        out.push(make(
            parent_id.clone(),
            account_name.clone(),
            account_name.clone(),
            FolderRole::Other,
            None,
            Vec::new(),
        ));
        for folder in rest {
            // Keep the member's hierarchy below the account node.
            let parent = folder
                .parent_id
                .as_ref()
                .filter(|p| rest_contains(&members, &used, p, account))
                .and_then(|p| by_id.get(p))
                .map_or_else(
                    || FolderId::new(parent_id.clone()),
                    |p| FolderId::new(format!("{parent_id}/{}", p.path)),
                );
            out.push(make(
                format!("{parent_id}/{}", folder.path),
                folder.name.clone(),
                format!("{account_name}/{}", folder.path),
                folder.role,
                Some(parent),
                vec![folder.id.clone()],
            ));
        }
    }
    out
}

fn rest_contains(
    members: &[&Folder],
    used: &HashSet<FolderId>,
    id: &FolderId,
    account: &AccountId,
) -> bool {
    !used.contains(id)
        && members
            .iter()
            .any(|f| &f.id == id && &f.account_id == account)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::accounts::{FolderMapping, FolderSource};

    fn folder(account: &str, path: &str, role: FolderRole, parent: Option<&str>) -> Folder {
        Folder {
            id: FolderId::new(format!("f-{account}-{path}")),
            account_id: account.into(),
            parent_id: parent.map(|p| FolderId::new(format!("f-{account}-{p}"))),
            name: path.rsplit('/').next().unwrap().into(),
            path: path.into(),
            role,
            unread: 1,
            total: 2,
        }
    }

    fn va(merge: bool) -> VirtualAccountConfig {
        VirtualAccountConfig {
            id: "both".into(),
            name: "Both".into(),
            members: vec!["work".into(), "home".into()],
            merge_roles: merge,
            folder_mappings: vec![FolderMapping {
                name: "Lists".into(),
                sources: vec![
                    FolderSource {
                        account_id: "work".into(),
                        path: "Lists".into(),
                    },
                    FolderSource {
                        account_id: "home".into(),
                        path: "Newsletters".into(),
                    },
                ],
            }],
            default_identity: None,
            identity_rules: vec![],
        }
    }

    fn folders() -> Vec<Folder> {
        vec![
            folder("work", "INBOX", FolderRole::Inbox, None),
            folder("work", "Sent", FolderRole::Sent, None),
            folder("work", "Lists", FolderRole::Other, None),
            folder("work", "Projects", FolderRole::Other, None),
            folder("work", "Projects/Nail", FolderRole::Other, Some("Projects")),
            folder("home", "INBOX", FolderRole::Inbox, None),
            folder("home", "Newsletters", FolderRole::Other, None),
            folder("other", "INBOX", FolderRole::Inbox, None),
        ]
    }

    #[test]
    fn merges_roles_mappings_and_keeps_the_rest() {
        let names = HashMap::from([(AccountId::new("work"), "Work".to_owned())]);
        let out = virtual_folders(&va(true), &folders(), &names);
        let ids: Vec<&str> = out.iter().map(|v| v.folder.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "both/inbox",
                "both/sent",
                "both/map-0",
                "both/account-work",
                "both/account-work/Projects",
                "both/account-work/Projects/Nail",
            ]
        );
        assert_eq!(
            out[0].sources,
            [FolderId::new("f-work-INBOX"), FolderId::new("f-home-INBOX")]
        );
        assert_eq!((out[0].folder.unread, out[0].folder.total), (2, 4));
        assert_eq!(out[0].folder.account_id, AccountId::new("both"));
        assert_eq!(out[2].sources.len(), 2);
        assert_eq!(
            out[5].folder.parent_id,
            Some(FolderId::new("both/account-work/Projects"))
        );
        assert_eq!(out[3].folder.name, "Work");
        assert_eq!(out[3].sources, Vec::<FolderId>::new());
        assert!(is_virtual_folder(&out[0].folder.id));
        assert!(!is_virtual_folder(&FolderId::new("f-1")));
        assert_eq!(
            virtual_account_of(&out[4].folder.id),
            Some(AccountId::new("both"))
        );
    }

    #[test]
    fn without_merging_every_account_gets_a_subtree() {
        let out = virtual_folders(&va(false), &folders(), &HashMap::new());
        assert!(
            out.iter()
                .any(|v| v.folder.id.as_str() == "both/account-home/INBOX")
        );
        assert!(out.iter().all(|v| v.folder.id.as_str() != "both/inbox"));
        let home = out
            .iter()
            .find(|v| v.folder.id.as_str() == "both/account-home")
            .unwrap();
        assert_eq!(home.folder.name, "home");
    }

    #[test]
    fn all_accounts_takes_enabled_and_local_accounts() {
        let config: AccountsConfig = nail_config::parse_accounts(
            "account \"work\" email=\"a@w.example\" { imap host=\"i\" }\n\
             account \"old\" email=\"a@o.example\" enabled=#false { imap host=\"i\" }\n",
            "accounts.kdl",
        )
        .unwrap();
        let stored = vec![
            Account {
                id: "demo".into(),
                name: "Demo".into(),
                email: String::new(),
                kind: AccountKind::Local,
            },
            Account {
                id: "gone".into(),
                name: "Gone".into(),
                email: String::new(),
                kind: AccountKind::Imap,
            },
        ];
        let all = all_accounts(&config, &stored);
        assert_eq!(
            all.members,
            [AccountId::new("work"), AccountId::new("demo")]
        );
        assert_eq!(all.id.as_str(), "all");
        assert!(virtual_config(&config, &stored, &AccountId::all()).is_some());
        assert!(virtual_config(&config, &stored, &"work".into()).is_none());
    }
}
