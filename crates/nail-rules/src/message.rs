//! What rules see of a message ([`MessageView`]) and of the accounts and
//! folders around it ([`Mailbox`]), plus the evaluation environment
//! ([`Env`]).
//!
//! The core implements both traits over its store (a stored message with
//! its parsed headers; its account and folder tables). [`MessageData`] and
//! [`StaticMailbox`] are plain in-memory implementations for tests, dry runs
//! over prepared data and tools.

use nail_api::rules::AddressField;
use nail_api::{AccountId, Address, Flags, FolderId, FolderRole, MessageId};

/// An attachment as rules see it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Attachment {
    /// File name, if the part has one.
    pub filename: Option<String>,
    /// MIME type, lowercase (`application/pdf`).
    pub mime: String,
}

/// A message as rules see it.
pub trait MessageView {
    /// The message.
    fn id(&self) -> &MessageId;
    /// The (real) account it belongs to.
    fn account_id(&self) -> &AccountId;
    /// The folder it is in.
    fn folder_id(&self) -> &FolderId;
    /// Every decoded value of a header (`name` is case-insensitive), in
    /// message order; empty when absent.
    fn header_values(&self, name: &str) -> Vec<String>;
    /// The addresses of a field.
    fn addresses(&self, field: AddressField) -> Vec<Address>;
    /// The decoded subject (empty when missing).
    fn subject(&self) -> String;
    /// The decoded body text; `None` when the body is not available (not
    /// downloaded). Body conditions do not match without a body.
    fn body_text(&self) -> Option<String>;
    /// The attachments.
    fn attachments(&self) -> Vec<Attachment>;
    /// Size in bytes.
    fn size(&self) -> u64;
    /// The message date, Unix seconds.
    fn date(&self) -> i64;
    /// System flags.
    fn flags(&self) -> Flags;
    /// Tags (IMAP keywords).
    fn tags(&self) -> Vec<String>;
}

/// A message held in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageData {
    /// The message.
    pub id: MessageId,
    /// Its account.
    pub account_id: AccountId,
    /// Its folder.
    pub folder_id: FolderId,
    /// Raw header list `(name, decoded value)` in message order.
    pub headers: Vec<(String, String)>,
    /// `From`.
    pub from: Vec<Address>,
    /// `To`.
    pub to: Vec<Address>,
    /// `Cc`.
    pub cc: Vec<Address>,
    /// `Reply-To`.
    pub reply_to: Vec<Address>,
    /// `Sender`.
    pub sender: Vec<Address>,
    /// Subject.
    pub subject: String,
    /// Body text, if available.
    pub body: Option<String>,
    /// Attachments.
    pub attachments: Vec<Attachment>,
    /// Size in bytes.
    pub size: u64,
    /// Date, Unix seconds.
    pub date: i64,
    /// Flags.
    pub flags: Flags,
    /// Tags.
    pub tags: Vec<String>,
}

impl MessageData {
    /// An empty message with these ids.
    pub fn new(
        id: impl Into<MessageId>,
        account_id: impl Into<AccountId>,
        folder_id: impl Into<FolderId>,
    ) -> Self {
        Self {
            id: id.into(),
            account_id: account_id.into(),
            folder_id: folder_id.into(),
            headers: Vec::new(),
            from: Vec::new(),
            to: Vec::new(),
            cc: Vec::new(),
            reply_to: Vec::new(),
            sender: Vec::new(),
            subject: String::new(),
            body: None,
            attachments: Vec::new(),
            size: 0,
            date: 0,
            flags: Flags::default(),
            tags: Vec::new(),
        }
    }
}

impl MessageView for MessageData {
    fn id(&self) -> &MessageId {
        &self.id
    }

    fn account_id(&self) -> &AccountId {
        &self.account_id
    }

    fn folder_id(&self) -> &FolderId {
        &self.folder_id
    }

    fn header_values(&self, name: &str) -> Vec<String> {
        self.headers
            .iter()
            .filter(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.clone())
            .collect()
    }

    fn addresses(&self, field: AddressField) -> Vec<Address> {
        match field {
            AddressField::From => self.from.clone(),
            AddressField::To => self.to.clone(),
            AddressField::Cc => self.cc.clone(),
            AddressField::Recipients => self.to.iter().chain(&self.cc).cloned().collect(),
            AddressField::ReplyTo => self.reply_to.clone(),
            AddressField::Sender => self.sender.clone(),
        }
    }

    fn subject(&self) -> String {
        self.subject.clone()
    }

    fn body_text(&self) -> Option<String> {
        self.body.clone()
    }

    fn attachments(&self) -> Vec<Attachment> {
        self.attachments.clone()
    }

    fn size(&self) -> u64 {
        self.size
    }

    fn date(&self) -> i64 {
        self.date
    }

    fn flags(&self) -> Flags {
        self.flags
    }

    fn tags(&self) -> Vec<String> {
        self.tags.clone()
    }
}

/// A folder as rules see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderInfo {
    /// The folder.
    pub id: FolderId,
    /// Its (real) account.
    pub account_id: AccountId,
    /// Full path, `/`-separated (`Lists/Rust`).
    pub path: String,
    /// Display name (last path segment).
    pub name: String,
    /// Special-use role.
    pub role: FolderRole,
}

/// An account as rules see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountInfo {
    /// The account.
    pub id: AccountId,
    /// Display name.
    pub name: String,
    /// Primary address (empty for virtual accounts).
    pub email: String,
    /// Members of a virtual account; empty for real accounts.
    pub members: Vec<AccountId>,
}

impl AccountInfo {
    /// Whether this is a virtual account.
    pub fn is_virtual(&self) -> bool {
        !self.members.is_empty()
    }
}

/// The accounts and folders rules refer to.
pub trait Mailbox {
    /// A folder by id.
    fn folder(&self, id: &FolderId) -> Option<FolderInfo>;
    /// The folders of a real account.
    fn folders(&self, account: &AccountId) -> Vec<FolderInfo>;
    /// An account (real or virtual) by id. The built-in "All accounts"
    /// ([`AccountId::ALL`]) need not be listed: it matches everything.
    fn account(&self, id: &AccountId) -> Option<AccountInfo>;
    /// Every account, real and virtual (for the query language's
    /// `account:` term, which also accepts names and addresses).
    fn accounts(&self) -> Vec<AccountInfo>;
}

/// Accounts and folders held in memory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StaticMailbox {
    /// Real and virtual accounts.
    pub accounts: Vec<AccountInfo>,
    /// Folders of the real accounts.
    pub folders: Vec<FolderInfo>,
}

impl Mailbox for StaticMailbox {
    fn folder(&self, id: &FolderId) -> Option<FolderInfo> {
        self.folders.iter().find(|f| &f.id == id).cloned()
    }

    fn folders(&self, account: &AccountId) -> Vec<FolderInfo> {
        self.folders
            .iter()
            .filter(|f| &f.account_id == account)
            .cloned()
            .collect()
    }

    fn account(&self, id: &AccountId) -> Option<AccountInfo> {
        self.accounts.iter().find(|a| &a.id == id).cloned()
    }

    fn accounts(&self) -> Vec<AccountInfo> {
        self.accounts.clone()
    }
}

/// Everything besides the message that an evaluation needs.
#[derive(Clone, Copy)]
pub struct Env<'a> {
    /// Now, Unix seconds (ages, relative dates).
    pub now: i64,
    /// The local time zone's offset east of UTC in seconds (calendar
    /// dates are local days).
    pub utc_offset_secs: i32,
    /// Accounts and folders.
    pub mailbox: &'a dyn Mailbox,
}

impl std::fmt::Debug for Env<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Env")
            .field("now", &self.now)
            .field("utc_offset_secs", &self.utc_offset_secs)
            .finish_non_exhaustive()
    }
}

impl Env<'_> {
    /// Whether a message of `message_account` belongs to `wanted`: the
    /// same account, a virtual account it is a member of, or "All
    /// accounts".
    pub fn account_matches(&self, message_account: &AccountId, wanted: &AccountId) -> bool {
        if message_account == wanted || wanted.as_str() == AccountId::ALL {
            return true;
        }
        let lower = wanted.as_str().to_lowercase();
        self.mailbox
            .account(wanted)
            .or_else(|| {
                // `virtual:Everything` may name a virtual account by its
                // display name.
                self.mailbox
                    .accounts()
                    .into_iter()
                    .find(|a| a.id.as_str() == lower || a.name.to_lowercase() == lower)
            })
            .is_some_and(|a| &a.id == message_account || a.members.contains(message_account))
    }

    /// The folder a [`nail_api::rules::FolderRef`] names for a message of
    /// `message_account`. A reference into a virtual account resolves in
    /// the message's account when it is a member.
    pub fn resolve_folder(
        &self,
        folder: &nail_api::rules::FolderRef,
        message_account: &AccountId,
    ) -> Option<FolderInfo> {
        use nail_api::rules::FolderRef;
        let named = match folder {
            FolderRef::Role { account_id, .. } | FolderRef::Path { account_id, .. } => {
                account_id.as_ref()
            }
        };
        let account = match named {
            None => message_account.clone(),
            Some(id) => match self.mailbox.account(id) {
                Some(info) if info.is_virtual() => {
                    if info.members.contains(message_account) {
                        message_account.clone()
                    } else {
                        info.members.first()?.clone()
                    }
                }
                _ => id.clone(),
            },
        };
        let folders = self.mailbox.folders(&account);
        match folder {
            FolderRef::Role { role, .. } => folders.into_iter().find(|f| f.role == *role),
            FolderRef::Path { path, .. } => {
                let wanted = path.trim_matches('/');
                folders
                    .iter()
                    .find(|f| f.path == wanted)
                    .or_else(|| folders.iter().find(|f| f.path.eq_ignore_ascii_case(wanted)))
                    .or_else(|| {
                        // "Inbox" names the inbox whatever its server path.
                        wanted
                            .eq_ignore_ascii_case("inbox")
                            .then(|| folders.iter().find(|f| f.role == FolderRole::Inbox))
                            .flatten()
                    })
                    .cloned()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::rules::FolderRef;

    fn mailbox() -> StaticMailbox {
        let folder = |id: &str, account: &str, path: &str, role| FolderInfo {
            id: id.into(),
            account_id: account.into(),
            path: path.into(),
            name: path.rsplit('/').next().unwrap_or(path).into(),
            role,
        };
        StaticMailbox {
            accounts: vec![
                AccountInfo {
                    id: "work".into(),
                    name: "Work".into(),
                    email: "me@work.example".into(),
                    members: vec![],
                },
                AccountInfo {
                    id: "home".into(),
                    name: "Home".into(),
                    email: "me@home.example".into(),
                    members: vec![],
                },
                AccountInfo {
                    id: "everything".into(),
                    name: "Everything".into(),
                    email: String::new(),
                    members: vec!["work".into(), "home".into()],
                },
            ],
            folders: vec![
                folder("w-in", "work", "INBOX", FolderRole::Inbox),
                folder("w-arch", "work", "Archive", FolderRole::Archive),
                folder("w-rust", "work", "Lists/Rust", FolderRole::Other),
                folder("h-in", "home", "INBOX", FolderRole::Inbox),
                folder("h-read", "home", "Reading", FolderRole::Other),
            ],
        }
    }

    #[test]
    fn accounts_match_directly_through_virtual_accounts_and_all() {
        let m = mailbox();
        let env = Env {
            now: 0,
            utc_offset_secs: 0,
            mailbox: &m,
        };
        let work = AccountId::new("work");
        assert!(env.account_matches(&work, &"work".into()));
        assert!(!env.account_matches(&work, &"home".into()));
        assert!(env.account_matches(&work, &"everything".into()));
        assert!(env.account_matches(&work, &"Everything".into()));
        assert!(env.account_matches(&work, &AccountId::new(AccountId::ALL)));
        assert!(!env.account_matches(&work, &"nobody".into()));
    }

    #[test]
    fn folders_resolve_by_role_path_and_account() {
        let m = mailbox();
        let env = Env {
            now: 0,
            utc_offset_secs: 0,
            mailbox: &m,
        };
        let work = AccountId::new("work");
        let role = |role, account: Option<&str>| FolderRef::Role {
            role,
            account_id: account.map(AccountId::new),
        };
        let path = |p: &str, account: Option<&str>| FolderRef::Path {
            path: p.into(),
            account_id: account.map(AccountId::new),
        };
        let id = |r: Option<FolderInfo>| r.map(|f| f.id.to_string());
        assert_eq!(
            id(env.resolve_folder(&role(FolderRole::Archive, None), &work)),
            Some("w-arch".into())
        );
        assert_eq!(
            id(env.resolve_folder(&role(FolderRole::Archive, Some("home")), &work)),
            None
        );
        assert_eq!(
            id(env.resolve_folder(&path("lists/rust", None), &work)),
            Some("w-rust".into())
        );
        assert_eq!(
            id(env.resolve_folder(&path("/Lists/Rust/", None), &work)),
            Some("w-rust".into())
        );
        assert_eq!(
            id(env.resolve_folder(&path("Inbox", None), &work)),
            Some("w-in".into())
        );
        assert_eq!(
            id(env.resolve_folder(&path("Reading", Some("home")), &work)),
            Some("h-read".into())
        );
        assert_eq!(
            id(env.resolve_folder(&path("Reading", Some("everything")), &"home".into())),
            Some("h-read".into())
        );
        assert_eq!(id(env.resolve_folder(&path("Nope", None), &work)), None);
    }

    #[test]
    fn message_data_answers_the_view() {
        let msg = MessageData {
            headers: vec![
                ("List-Id".into(), "<rust.lists.example>".into()),
                ("Received".into(), "a".into()),
                ("received".into(), "b".into()),
            ],
            to: vec![Address {
                name: None,
                email: "a@x".into(),
            }],
            cc: vec![Address {
                name: None,
                email: "b@x".into(),
            }],
            ..MessageData::new("m", "a", "f")
        };
        assert_eq!(msg.header_values("RECEIVED"), ["a", "b"]);
        assert!(msg.header_values("X-None").is_empty());
        assert_eq!(msg.addresses(AddressField::Recipients).len(), 2);
    }
}
