//! Mailbox settings: automatic replies (`GetUserOofSettings` /
//! `SetUserOofSettings`), inbox rules (`GetInboxRules` /
//! `UpdateInboxRules`) and the master category list (the `CategoryList`
//! user configuration on the calendar folder, an XML document in
//! `XmlData`).
//!
//! Inbox rules Nail cannot express completely (`IsNotSupported`, or
//! conditions / actions outside the contract's subset) are kept as the
//! server sent them: changing their `enabled` state writes the server's
//! own XML back with only `IsEnabled` replaced.

use std::collections::hash_map::RandomState;
use std::fmt::Write as _;
use std::hash::{BuildHasher, Hasher};

use base64::Engine as _;
use nail_api::exchange::engine::RemoteCategory;
use nail_api::exchange::oof::{AutomaticRepliesSettings, AutomaticRepliesStatus, ExternalAudience};
use nail_api::exchange::rules::{
    ExchangeInboxRule, ExchangeRuleActions, ExchangeRuleConditions, Importance,
};
use nail_api::exchange::{CategoryColor, ExchangeCategorySettings};

use crate::calendar::{importance, importance_name};
use crate::client::{EwsClient, Operation};
use crate::error::EwsError;
use crate::html::{html_to_text, text_to_html};
use crate::soap::{self, ResponseMessage};
use crate::types::{FolderRef, normalize_utc};
use crate::xml::{self, Element, el, escape};

/// The namespace of the category list document.
pub const CATEGORY_LIST_NS: &str = "CategoryList.xsd";

// ---------------------------------------------------------------------------
// Automatic replies

/// Reads `OofSettings` / `UserOofSettings`.
pub fn parse_oof(settings: &Element) -> AutomaticRepliesSettings {
    let status = match settings.child_text("OofState").unwrap_or("Disabled") {
        "Enabled" => AutomaticRepliesStatus::AlwaysEnabled,
        "Scheduled" => AutomaticRepliesStatus::Scheduled,
        _ => AutomaticRepliesStatus::Disabled,
    };
    let audience = match settings.child_text("ExternalAudience").unwrap_or("All") {
        "None" => ExternalAudience::None,
        "Known" => ExternalAudience::ContactsOnly,
        _ => ExternalAudience::All,
    };
    let duration = settings.child("Duration");
    let reply = |name: &str| {
        settings
            .child(name)
            .and_then(|r| r.child("Message"))
            .map(|m| html_to_text(&m.text))
            .unwrap_or_default()
    };
    AutomaticRepliesSettings {
        status,
        start: duration
            .and_then(|d| d.child_text("StartTime"))
            .and_then(normalize_utc),
        end: duration
            .and_then(|d| d.child_text("EndTime"))
            .and_then(normalize_utc),
        internal_reply: reply("InternalReply"),
        external_reply: reply("ExternalReply"),
        external_audience: audience,
    }
}

/// The `t:UserOofSettings` element of settings.
pub fn oof_xml(settings: &AutomaticRepliesSettings) -> String {
    let state = match settings.status {
        AutomaticRepliesStatus::Disabled => "Disabled",
        AutomaticRepliesStatus::AlwaysEnabled => "Enabled",
        AutomaticRepliesStatus::Scheduled => "Scheduled",
    };
    let audience = match settings.external_audience {
        ExternalAudience::None => "None",
        ExternalAudience::ContactsOnly => "Known",
        ExternalAudience::All => "All",
    };
    let duration = match (&settings.start, &settings.end) {
        (Some(start), Some(end)) => format!(
            "<t:Duration>{}{}</t:Duration>",
            el("t:StartTime", start),
            el("t:EndTime", end)
        ),
        _ => String::new(),
    };
    let reply = |tag: &str, text: &str| {
        format!(
            "<t:{tag}>{}</t:{tag}>",
            el(
                "t:Message",
                &if text.is_empty() {
                    String::new()
                } else {
                    text_to_html(text)
                }
            )
        )
    };
    format!(
        "<t:UserOofSettings>{}{}{duration}{}{}</t:UserOofSettings>",
        el("t:OofState", state),
        el("t:ExternalAudience", audience),
        reply("InternalReply", &settings.internal_reply),
        reply("ExternalReply", &settings.external_reply)
    )
}

fn mailbox_xml(address: &str) -> String {
    format!("<t:Mailbox>{}</t:Mailbox>", el("t:Address", address))
}

/// The answer element of the availability-style operations, which carry
/// one `ResponseMessage` instead of `ResponseMessages`.
fn check_single_response(body: &Element) -> Result<(), EwsError> {
    match body.child("ResponseMessage") {
        Some(message) => ResponseMessage::read(message).check(),
        None => ResponseMessage::read(body).check(),
    }
}

// ---------------------------------------------------------------------------
// Inbox rules

fn strings_xml(tag: &str, values: &[String]) -> String {
    if values.is_empty() {
        return String::new();
    }
    let inner: String = values.iter().map(|v| el("t:String", v)).collect();
    format!("<t:{tag}>{inner}</t:{tag}>")
}

fn addresses_xml(tag: &str, values: &[String]) -> String {
    if values.is_empty() {
        return String::new();
    }
    let inner: String = values.iter().fold(String::new(), |mut out, v| {
        let _ = write!(out, "<t:Address>{}</t:Address>", el("t:EmailAddress", v));
        out
    });
    format!("<t:{tag}>{inner}</t:{tag}>")
}

fn bool_xml(tag: &str, value: Option<bool>) -> String {
    value
        .map(|v| el(&format!("t:{tag}"), if v { "true" } else { "false" }))
        .unwrap_or_default()
}

/// The `RulePredicatesType` of conditions (schema order).
pub fn conditions_xml(tag: &str, c: &ExchangeRuleConditions) -> String {
    if c.is_empty() {
        return String::new();
    }
    let size = if c.min_size_kb.is_some() || c.max_size_kb.is_some() {
        format!(
            "<t:WithinSizeRange>{}{}</t:WithinSizeRange>",
            c.min_size_kb
                .map(|v| el("t:MinimumSize", &v.to_string()))
                .unwrap_or_default(),
            c.max_size_kb
                .map(|v| el("t:MaximumSize", &v.to_string()))
                .unwrap_or_default()
        )
    } else {
        String::new()
    };
    let parts = [
        strings_xml("Categories", &c.categories),
        strings_xml("ContainsBodyStrings", &c.body_contains),
        strings_xml("ContainsHeaderStrings", &c.header_contains),
        strings_xml("ContainsRecipientStrings", &c.recipient_contains),
        strings_xml("ContainsSenderStrings", &c.sender_contains),
        strings_xml("ContainsSubjectOrBodyStrings", &c.body_or_subject_contains),
        strings_xml("ContainsSubjectStrings", &c.subject_contains),
        addresses_xml("FromAddresses", &c.from_addresses),
        bool_xml("HasAttachments", c.has_attachments),
        c.importance
            .map(|i| el("t:Importance", importance_name(i)))
            .unwrap_or_default(),
        bool_xml("IsMeetingRequest", c.is_meeting_request),
        bool_xml("NotSentToMe", c.not_sent_to_me),
        bool_xml("SentCcMe", c.sent_cc_me),
        bool_xml("SentOnlyToMe", c.sent_only_to_me),
        addresses_xml("SentToAddresses", &c.sent_to_addresses),
        bool_xml("SentToMe", c.sent_to_me),
        bool_xml("SentToOrCcMe", c.sent_to_or_cc_me),
        size,
    ];
    format!("<t:{tag}>{}</t:{tag}>", parts.concat())
}

/// The `RuleActionsType` of actions (schema order). Folder ids are the
/// folders' remote ids at the engine seam.
pub fn actions_xml(a: &ExchangeRuleActions) -> String {
    let folder = |tag: &str, id: &Option<nail_api::FolderId>| {
        id.as_ref()
            .map(|id| {
                format!(
                    r#"<t:{tag}><t:FolderId Id="{}"/></t:{tag}>"#,
                    escape(id.as_str())
                )
            })
            .unwrap_or_default()
    };
    let flag = |tag: &str, on: bool| {
        if on {
            el(&format!("t:{tag}"), "true")
        } else {
            String::new()
        }
    };
    let parts = [
        strings_xml("AssignCategories", &a.assign_categories),
        folder("CopyToFolder", &a.copy_to_folder),
        flag("Delete", a.delete),
        addresses_xml(
            "ForwardAsAttachmentToRecipients",
            &a.forward_as_attachment_to,
        ),
        addresses_xml("ForwardToRecipients", &a.forward_to),
        a.mark_importance
            .map(|i| el("t:MarkImportance", importance_name(i)))
            .unwrap_or_default(),
        flag("MarkAsRead", a.mark_as_read),
        folder("MoveToFolder", &a.move_to_folder),
        flag("PermanentDelete", a.permanent_delete),
        addresses_xml("RedirectToRecipients", &a.redirect_to),
        flag("StopProcessingRules", a.stop_processing_rules),
    ];
    format!("<t:Actions>{}</t:Actions>", parts.concat())
}

/// The `t:Rule` element of a rule (`with_id` for changes).
pub fn rule_xml(rule: &ExchangeInboxRule, with_id: bool) -> String {
    let id = if with_id {
        el("t:RuleId", &rule.id)
    } else {
        String::new()
    };
    format!(
        "<t:Rule>{id}{}{}{}{}{}{}</t:Rule>",
        el("t:DisplayName", &rule.name),
        el("t:Priority", &rule.sequence.max(1).to_string()),
        el("t:IsEnabled", if rule.enabled { "true" } else { "false" }),
        conditions_xml("Conditions", &rule.conditions),
        conditions_xml("Exceptions", &rule.exceptions),
        actions_xml(&rule.actions)
    )
}

const KNOWN_CONDITIONS: [&str; 18] = [
    "Categories",
    "ContainsBodyStrings",
    "ContainsHeaderStrings",
    "ContainsRecipientStrings",
    "ContainsSenderStrings",
    "ContainsSubjectOrBodyStrings",
    "ContainsSubjectStrings",
    "FromAddresses",
    "HasAttachments",
    "Importance",
    "IsMeetingRequest",
    "NotSentToMe",
    "SentCcMe",
    "SentOnlyToMe",
    "SentToAddresses",
    "SentToMe",
    "SentToOrCcMe",
    "WithinSizeRange",
];

const KNOWN_ACTIONS: [&str; 11] = [
    "AssignCategories",
    "CopyToFolder",
    "Delete",
    "ForwardAsAttachmentToRecipients",
    "ForwardToRecipients",
    "MarkImportance",
    "MarkAsRead",
    "MoveToFolder",
    "PermanentDelete",
    "RedirectToRecipients",
    "StopProcessingRules",
];

fn addresses(element: &Element, tag: &str) -> Vec<String> {
    element
        .child(tag)
        .map(|list| {
            list.children
                .iter()
                .filter_map(|a| a.child_value("EmailAddress"))
                .collect()
        })
        .unwrap_or_default()
}

/// Reads conditions; answers whether something outside the subset was
/// there.
fn parse_conditions(element: Option<&Element>) -> (ExchangeRuleConditions, bool) {
    let Some(c) = element else {
        return (ExchangeRuleConditions::default(), false);
    };
    let unsupported = c
        .children
        .iter()
        .any(|child| !KNOWN_CONDITIONS.contains(&child.name.as_str()));
    let size = c.child("WithinSizeRange");
    (
        ExchangeRuleConditions {
            from_addresses: addresses(c, "FromAddresses"),
            sent_to_addresses: addresses(c, "SentToAddresses"),
            subject_contains: c.strings("ContainsSubjectStrings", "String"),
            body_contains: c.strings("ContainsBodyStrings", "String"),
            body_or_subject_contains: c.strings("ContainsSubjectOrBodyStrings", "String"),
            header_contains: c.strings("ContainsHeaderStrings", "String"),
            sender_contains: c.strings("ContainsSenderStrings", "String"),
            recipient_contains: c.strings("ContainsRecipientStrings", "String"),
            categories: c.strings("Categories", "String"),
            has_attachments: c.child_bool("HasAttachments"),
            importance: c.child_text("Importance").and_then(importance),
            sent_to_me: c.child_bool("SentToMe"),
            sent_only_to_me: c.child_bool("SentOnlyToMe"),
            sent_cc_me: c.child_bool("SentCcMe"),
            sent_to_or_cc_me: c.child_bool("SentToOrCcMe"),
            not_sent_to_me: c.child_bool("NotSentToMe"),
            is_meeting_request: c.child_bool("IsMeetingRequest"),
            min_size_kb: size.and_then(|s| s.child_number("MinimumSize")),
            max_size_kb: size.and_then(|s| s.child_number("MaximumSize")),
        },
        unsupported,
    )
}

fn parse_actions(element: Option<&Element>) -> (ExchangeRuleActions, bool) {
    let Some(a) = element else {
        return (ExchangeRuleActions::default(), false);
    };
    let unsupported = a
        .children
        .iter()
        .any(|child| !KNOWN_ACTIONS.contains(&child.name.as_str()));
    let folder = |tag: &str| {
        a.child(tag)
            .and_then(|f| {
                f.child("FolderId")
                    .or_else(|| f.child("DistinguishedFolderId"))
            })
            .and_then(|f| f.attr("Id"))
            .map(nail_api::FolderId::new)
    };
    (
        ExchangeRuleActions {
            move_to_folder: folder("MoveToFolder"),
            copy_to_folder: folder("CopyToFolder"),
            delete: a.child_bool("Delete").unwrap_or(false),
            permanent_delete: a.child_bool("PermanentDelete").unwrap_or(false),
            mark_as_read: a.child_bool("MarkAsRead").unwrap_or(false),
            mark_importance: a.child_text("MarkImportance").and_then(importance),
            assign_categories: a.strings("AssignCategories", "String"),
            forward_to: addresses(a, "ForwardToRecipients"),
            forward_as_attachment_to: addresses(a, "ForwardAsAttachmentToRecipients"),
            redirect_to: addresses(a, "RedirectToRecipients"),
            stop_processing_rules: a.child_bool("StopProcessingRules").unwrap_or(false),
        },
        unsupported,
    )
}

/// A `t:Rule` element as an [`ExchangeInboxRule`].
pub fn parse_rule(rule: &Element) -> Option<ExchangeInboxRule> {
    let (conditions, odd_conditions) = parse_conditions(rule.child("Conditions"));
    let (exceptions, odd_exceptions) = parse_conditions(rule.child("Exceptions"));
    let (actions, odd_actions) = parse_actions(rule.child("Actions"));
    Some(ExchangeInboxRule {
        id: rule.child_value("RuleId")?,
        name: rule
            .child_text("DisplayName")
            .unwrap_or_default()
            .to_owned(),
        sequence: rule.child_number("Priority").unwrap_or(1),
        enabled: rule.child_bool("IsEnabled").unwrap_or(false),
        conditions,
        exceptions,
        actions,
        has_unsupported: rule.child_bool("IsNotSupported").unwrap_or(false)
            || odd_conditions
            || odd_exceptions
            || odd_actions,
        in_error: rule.child_bool("IsInError").unwrap_or(false),
        nail_rule_id: None,
    })
}

/// One `UpdateInboxRules` operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleOperation {
    /// `CreateRuleOperation`.
    Create(ExchangeInboxRule),
    /// `SetRuleOperation` with Nail's version of the rule.
    Set(ExchangeInboxRule),
    /// `SetRuleOperation` with the server's own rule XML (unsupported
    /// parts kept).
    SetRaw(Element),
    /// `DeleteRuleOperation`.
    Delete(String),
}

impl RuleOperation {
    fn xml(&self) -> String {
        match self {
            Self::Create(rule) => format!(
                "<t:CreateRuleOperation>{}</t:CreateRuleOperation>",
                rule_xml(rule, false)
            ),
            Self::Set(rule) => format!(
                "<t:SetRuleOperation>{}</t:SetRuleOperation>",
                rule_xml(rule, true)
            ),
            Self::SetRaw(rule) => {
                let mut rule = rule.clone();
                // Read-only flags are not written back.
                rule.children
                    .retain(|c| c.name != "IsNotSupported" && c.name != "IsInError");
                format!(
                    "<t:SetRuleOperation>{}</t:SetRuleOperation>",
                    rule.to_xml(Some("t"))
                )
            }
            Self::Delete(id) => format!(
                "<t:DeleteRuleOperation>{}</t:DeleteRuleOperation>",
                el("t:RuleId", id)
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Categories

/// A new category GUID (`{xxxxxxxx-xxxx-4xxx-8xxx-xxxxxxxxxxxx}`).
pub fn new_guid() -> String {
    let mut bytes = [0u8; 16];
    for (i, chunk) in bytes.chunks_mut(8).enumerate() {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_usize(i);
        hasher.write_u128(nail_util::time::unix_now_nanos());
        chunk.copy_from_slice(&hasher.finish().to_le_bytes());
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    });
    format!(
        "{{{}-{}-{}-{}-{}}}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// The categories of a category list document.
pub fn parse_category_list(root: &Element) -> Vec<RemoteCategory> {
    root.children_named("category")
        .filter_map(|c| {
            Some(RemoteCategory {
                id: c.attr("guid")?.to_owned(),
                name: c.attr("name")?.to_owned(),
                color: CategoryColor::from_ews_index(
                    c.attr("color").and_then(|v| v.parse().ok()).unwrap_or(-1),
                ),
            })
        })
        .collect()
}

fn ids_match(a: &str, b: &str) -> bool {
    a.trim_matches(['{', '}'])
        .eq_ignore_ascii_case(b.trim_matches(['{', '}']))
}

/// An empty category list document.
pub fn empty_category_list() -> Element {
    let mut root = Element::new("categories");
    CATEGORY_LIST_NS.clone_into(&mut root.ns);
    root.set_attr("default", "");
    root.set_attr("lastSavedSession", "0");
    root
}

/// A category list change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CategoryEdit<'a> {
    /// Add a category with this id.
    Create(&'a str, &'a ExchangeCategorySettings),
    /// Change name / colour.
    Update(&'a str, &'a ExchangeCategorySettings),
    /// Remove.
    Delete(&'a str),
}

/// Applies an edit to a category list document; answers whether it
/// changed (deleting an unknown id changes nothing; updating one is
/// [`EwsError::NotFound`]; a duplicate name is [`EwsError::Conflict`]).
pub fn edit_category_list(root: &mut Element, edit: &CategoryEdit<'_>) -> Result<bool, EwsError> {
    let name_taken = |root: &Element, name: &str, except: Option<&str>| {
        root.children_named("category").any(|c| {
            c.attr("name").is_some_and(|n| n.eq_ignore_ascii_case(name))
                && !except.is_some_and(|id| c.attr("guid").is_some_and(|g| ids_match(g, id)))
        })
    };
    match edit {
        CategoryEdit::Create(id, settings) => {
            if name_taken(root, &settings.name, None) {
                return Err(EwsError::Conflict(format!("category {}", settings.name)));
            }
            let mut category = Element::new("category");
            category.ns.clone_from(&root.ns);
            category.set_attr("name", settings.name.clone());
            category.set_attr("color", settings.color.ews_index().to_string());
            category.set_attr("keyboardShortcut", "0");
            category.set_attr("usageCount", "0");
            category.set_attr("guid", *id);
            category.set_attr("renameOnFirstUse", "0");
            root.children.push(category);
            Ok(true)
        }
        CategoryEdit::Update(id, settings) => {
            let exists = root
                .children
                .iter()
                .any(|c| c.name == "category" && c.attr("guid").is_some_and(|g| ids_match(g, id)));
            if !exists {
                return Err(EwsError::NotFound(format!("category {id}")));
            }
            if name_taken(root, &settings.name, Some(id)) {
                return Err(EwsError::Conflict(format!("category {}", settings.name)));
            }
            if let Some(category) = root
                .children
                .iter_mut()
                .find(|c| c.name == "category" && c.attr("guid").is_some_and(|g| ids_match(g, id)))
            {
                category.set_attr("name", settings.name.clone());
                category.set_attr("color", settings.color.ews_index().to_string());
            }
            Ok(true)
        }
        CategoryEdit::Delete(id) => {
            let before = root.children.len();
            root.children.retain(|c| {
                !(c.name == "category" && c.attr("guid").is_some_and(|g| ids_match(g, id)))
            });
            Ok(root.children.len() != before)
        }
    }
}

impl EwsClient {
    fn own_mailbox(&self) -> Result<String, EwsError> {
        self.own_address()
            .map(str::to_owned)
            .ok_or_else(|| EwsError::Config("the account's address is not known".into()))
    }

    /// `GetUserOofSettings` of the own mailbox.
    pub async fn get_oof(&self) -> Result<AutomaticRepliesSettings, EwsError> {
        let body = format!(
            "<m:GetUserOofSettingsRequest>{}</m:GetUserOofSettingsRequest>",
            mailbox_xml(&self.own_mailbox()?)
        );
        let response = self
            .call(&Operation::read("GetUserOofSettings", body))
            .await?;
        check_single_response(&response.body)?;
        let settings = response
            .body
            .child("OofSettings")
            .ok_or_else(|| EwsError::Protocol("GetUserOofSettings without settings".into()))?;
        Ok(parse_oof(settings))
    }

    /// `SetUserOofSettings` of the own mailbox.
    pub async fn set_oof(&self, settings: &AutomaticRepliesSettings) -> Result<(), EwsError> {
        let body = format!(
            "<m:SetUserOofSettingsRequest>{}{}</m:SetUserOofSettingsRequest>",
            mailbox_xml(&self.own_mailbox()?),
            oof_xml(settings)
        );
        let response = self
            .call(&Operation::read("SetUserOofSettings", body))
            .await?;
        check_single_response(&response.body)
    }

    /// `GetInboxRules`: the rules as Nail reads them and as the server
    /// sent them (for writing unsupported rules back), by priority.
    pub async fn get_inbox_rules(&self) -> Result<Vec<(ExchangeInboxRule, Element)>, EwsError> {
        let body = format!(
            "<m:GetInboxRules>{}</m:GetInboxRules>",
            el("m:MailboxSmtpAddress", &self.own_mailbox()?)
        );
        let response = self.call(&Operation::read("GetInboxRules", body)).await?;
        ResponseMessage::read(&response.body).check()?;
        let mut rules: Vec<(ExchangeInboxRule, Element)> = response
            .body
            .child("InboxRules")
            .map(|list| {
                list.children_named("Rule")
                    .filter_map(|r| parse_rule(r).map(|rule| (rule, r.clone())))
                    .collect()
            })
            .unwrap_or_default();
        rules.sort_by_key(|(r, _)| r.sequence);
        Ok(rules)
    }

    /// `UpdateInboxRules` with operations. Outlook's client-only rules are
    /// kept: a server that has them answers
    /// [`EwsError::OutlookRuleBlob`] unless `remove_outlook_rules`.
    pub async fn update_inbox_rules(
        &self,
        operations: &[RuleOperation],
        remove_outlook_rules: bool,
    ) -> Result<(), EwsError> {
        let ops: String = operations.iter().map(RuleOperation::xml).collect();
        let body = format!(
            "<m:UpdateInboxRules>{}{}<m:Operations>{ops}</m:Operations></m:UpdateInboxRules>",
            el("m:MailboxSmtpAddress", &self.own_mailbox()?),
            el(
                "m:RemoveOutlookRuleBlob",
                if remove_outlook_rules {
                    "true"
                } else {
                    "false"
                }
            )
        );
        let response = self
            .call(&Operation::write("UpdateInboxRules", body))
            .await?;
        let message = ResponseMessage::read(&response.body);
        if message.is_error() {
            let detail = response
                .body
                .find("RuleOperationErrors")
                .and_then(|e| e.find("ErrorMessage"))
                .map(|m| m.text.trim().to_owned());
            return Err(match (message.error(), detail) {
                (EwsError::Rejected { code, message }, Some(detail)) => EwsError::Rejected {
                    code,
                    message: format!("{message} {detail}").trim().to_owned(),
                },
                (error, _) => error,
            });
        }
        Ok(())
    }

    /// The category list document (an empty one when the mailbox has
    /// none yet) and whether it exists on the server.
    pub async fn get_category_list(&self) -> Result<(Element, bool), EwsError> {
        let body = format!(
            r#"<m:GetUserConfiguration><m:UserConfigurationName Name="CategoryList">{}</m:UserConfigurationName><m:UserConfigurationProperties>XmlData</m:UserConfigurationProperties></m:GetUserConfiguration>"#,
            FolderRef::distinguished("calendar", None).xml()
        );
        let response = self
            .call(&Operation::read("GetUserConfiguration", body))
            .await?;
        match soap::single_message(&response.body) {
            Ok(message) => {
                let data = message
                    .find("XmlData")
                    .map(|d| d.text.trim().to_owned())
                    .unwrap_or_default();
                if data.is_empty() {
                    return Ok((empty_category_list(), true));
                }
                let bytes = crate::mail::decode_base64(&data)?;
                let text = String::from_utf8_lossy(&bytes);
                Ok((xml::parse(&text)?, true))
            }
            Err(EwsError::NotFound(_)) => Ok((empty_category_list(), false)),
            Err(EwsError::Rejected { code, .. }) if code == "ErrorItemNotFound" => {
                Ok((empty_category_list(), false))
            }
            Err(e) => Err(e),
        }
    }

    /// Writes the category list document (`UpdateUserConfiguration`, or
    /// `CreateUserConfiguration` when it does not exist yet).
    pub async fn put_category_list(&self, root: &Element, exists: bool) -> Result<(), EwsError> {
        let data = base64::engine::general_purpose::STANDARD.encode(root.to_document());
        let (op, what) = if exists {
            ("UpdateUserConfiguration", "UpdateUserConfiguration")
        } else {
            ("CreateUserConfiguration", "CreateUserConfiguration")
        };
        let body = format!(
            r#"<m:{op}><m:UserConfiguration><t:UserConfigurationName Name="CategoryList">{}</t:UserConfigurationName><t:XmlData>{data}</t:XmlData></m:UserConfiguration></m:{op}>"#,
            FolderRef::distinguished("calendar", None).xml()
        );
        let response = self.call(&Operation::write(what, body)).await?;
        soap::single_message(&response.body)?;
        Ok(())
    }

    /// Reads, edits and writes the category list.
    pub async fn edit_categories(&self, edit: &CategoryEdit<'_>) -> Result<(), EwsError> {
        let (mut root, exists) = self.get_category_list().await?;
        if edit_category_list(&mut root, edit)? {
            self.put_category_list(&root, exists).await?;
        }
        Ok(())
    }
}

/// Whether two rules are the same for Nail (used to find a created rule's
/// id: `UpdateInboxRules` does not answer it).
pub fn same_rule(a: &ExchangeInboxRule, b: &ExchangeInboxRule) -> bool {
    a.name == b.name
        && a.conditions == b.conditions
        && a.actions == b.actions
        && a.exceptions == b.exceptions
}

/// Importance names, re-exported for the mock server.
pub fn importance_text(importance: Importance) -> &'static str {
    importance_name(importance)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::parse;
    use pretty_assertions::assert_eq;

    #[test]
    fn oof_round_trips() {
        let settings = AutomaticRepliesSettings {
            status: AutomaticRepliesStatus::Scheduled,
            start: Some("2026-10-05T07:00:00Z".into()),
            end: Some("2026-10-09T16:00:00Z".into()),
            internal_reply: "Back on Monday.\nAsk Bob.".into(),
            external_reply: "I am out & about.".into(),
            external_audience: ExternalAudience::ContactsOnly,
        };
        let xml = oof_xml(&settings);
        assert!(xml.contains("<t:OofState>Scheduled</t:OofState>"));
        assert!(xml.contains("<t:ExternalAudience>Known</t:ExternalAudience>"));
        let element = parse(&format!(r#"<x xmlns:t="t">{xml}</x>"#)).unwrap();
        assert_eq!(
            parse_oof(element.child("UserOofSettings").unwrap()),
            settings
        );
        let off = parse(
            "<OofSettings><OofState>Disabled</OofState><ExternalAudience>All</ExternalAudience><InternalReply><Message/></InternalReply></OofSettings>",
        )
        .unwrap();
        assert_eq!(parse_oof(&off), AutomaticRepliesSettings::default());
    }

    #[test]
    fn rules_round_trip() {
        let rule = ExchangeInboxRule {
            id: "AQAAAJ5".into(),
            name: "Newsletters".into(),
            sequence: 2,
            enabled: true,
            conditions: ExchangeRuleConditions {
                sender_contains: vec!["@substack.com".into()],
                from_addresses: vec!["news@corp.example".into()],
                has_attachments: Some(false),
                importance: Some(Importance::Low),
                min_size_kb: Some(10),
                ..ExchangeRuleConditions::default()
            },
            exceptions: ExchangeRuleConditions {
                subject_contains: vec!["urgent".into()],
                ..ExchangeRuleConditions::default()
            },
            actions: ExchangeRuleActions {
                move_to_folder: Some("F-reading".into()),
                assign_categories: vec!["newsletter".into()],
                mark_as_read: true,
                stop_processing_rules: true,
                forward_to: vec!["ada@home.example".into()],
                ..ExchangeRuleActions::default()
            },
            has_unsupported: false,
            in_error: false,
            nail_rule_id: None,
        };
        let xml = rule_xml(&rule, true);
        let element = parse(&format!(r#"<x xmlns:t="t">{xml}</x>"#)).unwrap();
        let back = parse_rule(element.child("Rule").unwrap()).unwrap();
        assert_eq!(back, rule);
        assert!(same_rule(&back, &rule));
        assert!(!rule_xml(&rule, false).contains("RuleId"));
        let actions = actions_xml(&rule.actions);
        assert!(
            actions.find("AssignCategories").unwrap()
                < actions.find("ForwardToRecipients").unwrap()
        );
        assert!(actions.find("MarkAsRead").unwrap() < actions.find("MoveToFolder").unwrap());
    }

    #[test]
    fn unsupported_rules_are_kept_raw() {
        let raw = parse(concat!(
            r#"<t:Rule xmlns:t="t"><t:RuleId>R9</t:RuleId><t:DisplayName>Odd</t:DisplayName><t:Priority>1</t:Priority>"#,
            r#"<t:IsEnabled>true</t:IsEnabled><t:IsNotSupported>false</t:IsNotSupported><t:IsInError>false</t:IsInError>"#,
            r#"<t:Conditions><t:IsVoicemail>true</t:IsVoicemail></t:Conditions>"#,
            r#"<t:Actions><t:ServerReplyWithMessage><t:ItemId Id="X"/></t:ServerReplyWithMessage></t:Actions></t:Rule>"#
        ))
        .unwrap();
        let rule = parse_rule(&raw).unwrap();
        assert!(rule.has_unsupported);
        let mut edited = raw.clone();
        edited.child_mut("IsEnabled").unwrap().text = "false".into();
        let xml = RuleOperation::SetRaw(edited).xml();
        assert!(xml.contains("<t:IsEnabled>false</t:IsEnabled>"));
        assert!(xml.contains("<t:IsVoicemail>true</t:IsVoicemail>"));
        assert!(!xml.contains("IsNotSupported"));
        assert!(
            RuleOperation::Delete("R9".into())
                .xml()
                .contains("<t:RuleId>R9</t:RuleId>")
        );
    }

    #[test]
    fn category_lists() {
        let doc = concat!(
            r#"<?xml version="1.0"?><categories default="Red Category" lastSavedSession="2" xmlns="CategoryList.xsd">"#,
            r#"<category name="Red Category" color="0" keyboardShortcut="0" usageCount="3" guid="{4e66b8b4-0000-4000-8000-000000000001}" renameOnFirstUse="1"/>"#,
            r#"<category name="Work" color="7" guid="{4e66b8b4-0000-4000-8000-000000000002}"/></categories>"#
        );
        let mut root = parse(doc).unwrap();
        let list = parse_category_list(&root);
        assert_eq!(list.len(), 2);
        assert_eq!(list[1].color, CategoryColor::Preset7);
        let guid = new_guid();
        assert_eq!(guid.len(), 38);
        assert_ne!(guid, new_guid());
        let blue = ExchangeCategorySettings {
            name: "Travel".into(),
            color: CategoryColor::Preset4,
        };
        assert!(edit_category_list(&mut root, &CategoryEdit::Create(&guid, &blue)).unwrap());
        assert!(matches!(
            edit_category_list(
                &mut root,
                &CategoryEdit::Create("x", &ExchangeCategorySettings::named("work"))
            ),
            Err(EwsError::Conflict(_))
        ));
        let renamed = ExchangeCategorySettings {
            name: "Trips".into(),
            color: CategoryColor::None,
        };
        let id = guid.trim_matches(['{', '}']).to_uppercase();
        assert!(edit_category_list(&mut root, &CategoryEdit::Update(&id, &renamed)).unwrap());
        assert!(matches!(
            edit_category_list(&mut root, &CategoryEdit::Update("missing", &renamed)),
            Err(EwsError::NotFound(_))
        ));
        let reparsed = parse(&root.to_document()).unwrap();
        let list = parse_category_list(&reparsed);
        assert_eq!(list[2].name, "Trips");
        assert_eq!(list[2].color, CategoryColor::None);
        assert_eq!(reparsed.children[0].attr("usageCount"), Some("3"));
        assert!(edit_category_list(&mut root, &CategoryEdit::Delete(&guid)).unwrap());
        assert!(!edit_category_list(&mut root, &CategoryEdit::Delete(&guid)).unwrap());
        assert_eq!(
            empty_category_list()
                .to_document()
                .contains("CategoryList.xsd"),
            true
        );
    }
}
