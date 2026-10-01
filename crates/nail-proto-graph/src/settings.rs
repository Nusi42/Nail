//! Mailbox settings: the master category list
//! (`outlook/masterCategories`), automatic replies
//! (`mailboxSettings/automaticRepliesSetting`, HTML ↔ plain text) and inbox
//! rules (`mailFolders/inbox/messageRules` ↔ [`ExchangeInboxRule`]).

use nail_api::FolderId;
use nail_api::exchange::engine::RemoteCategory;
use nail_api::exchange::oof::{AutomaticRepliesStatus, ExternalAudience};
use nail_api::exchange::rules::{
    ExchangeInboxRule, ExchangeRuleActions, ExchangeRuleConditions, Importance,
};
use nail_api::exchange::{AutomaticRepliesSettings, CategoryColor, ExchangeCategorySettings};
use reqwest::Method;
use serde_json::{Map, Value, json};

use crate::engine::GraphEngine;
use crate::error::GraphError;
use crate::wire::{AutomaticRepliesSetting, DateTimeTimeZone, OutlookCategory, Page};
use crate::{mailbox_path, path_segment, text};

const CATEGORIES: &str = "/me/outlook/masterCategories";
const RULES: &str = "/me/mailFolders/inbox/messageRules";

/// The predicates Nail reads and writes; any other set predicate makes a
/// rule [`ExchangeInboxRule::has_unsupported`].
const KNOWN_PREDICATES: [&str; 19] = [
    "fromAddresses",
    "sentToAddresses",
    "subjectContains",
    "bodyContains",
    "bodyOrSubjectContains",
    "headerContains",
    "senderContains",
    "recipientContains",
    "categories",
    "hasAttachments",
    "importance",
    "sentToMe",
    "sentOnlyToMe",
    "sentCcMe",
    "sentToOrCcMe",
    "notSentToMe",
    "isMeetingRequest",
    "withinSizeRange",
    "@odata.type",
];

/// The actions Nail reads and writes.
const KNOWN_ACTIONS: [&str; 12] = [
    "moveToFolder",
    "copyToFolder",
    "delete",
    "permanentDelete",
    "markAsRead",
    "markImportance",
    "assignCategories",
    "forwardTo",
    "forwardAsAttachmentTo",
    "redirectTo",
    "stopProcessingRules",
    "@odata.type",
];

fn category(c: OutlookCategory) -> RemoteCategory {
    RemoteCategory {
        id: c.id,
        name: c.display_name,
        color: c
            .color
            .as_deref()
            .and_then(CategoryColor::parse)
            .unwrap_or_default(),
    }
}

impl GraphEngine {
    /// The master category list.
    pub async fn master_categories(&self) -> Result<Vec<RemoteCategory>, GraphError> {
        let page: Page<OutlookCategory> = self
            .client
            .get_json(CATEGORIES, &[], &[], "GET masterCategories")
            .await?;
        Ok(page.value.into_iter().map(category).collect())
    }

    async fn category_named(&self, name: &str) -> Result<Option<RemoteCategory>, GraphError> {
        Ok(self
            .master_categories()
            .await?
            .into_iter()
            .find(|c| c.name.eq_ignore_ascii_case(name)))
    }

    /// Creates a category; answers its id. Replaying a create finds the
    /// category of that name (Graph answers `409`).
    pub async fn add_category(
        &self,
        settings: &ExchangeCategorySettings,
    ) -> Result<String, GraphError> {
        let created = self
            .client
            .send_json(
                Method::POST,
                CATEGORIES,
                Some(&json!({
                    "displayName": settings.name,
                    "color": settings.color.as_str(),
                })),
                &[],
                false,
                "POST masterCategories",
            )
            .await;
        match created {
            Ok(value) => {
                let c: OutlookCategory = serde_json::from_value(value)
                    .map_err(|e| GraphError::Protocol(format!("category: {e}")))?;
                Ok(c.id)
            }
            Err(GraphError::Conflict(message)) => self
                .category_named(&settings.name)
                .await?
                .map(|c| c.id)
                .ok_or(GraphError::Conflict(message)),
            Err(e) => Err(e),
        }
    }

    /// Changes a category. Graph cannot rename one (`displayName` is fixed
    /// once created): a new name creates the new category and deletes the
    /// old one, and the answer is the new id (`None` = same id).
    pub async fn change_category(
        &self,
        id: &str,
        settings: &ExchangeCategorySettings,
    ) -> Result<Option<String>, GraphError> {
        let current = self
            .master_categories()
            .await?
            .into_iter()
            .find(|c| c.id == id);
        match current {
            Some(c) if c.name == settings.name => {
                if c.color != settings.color {
                    self.client
                        .send_json(
                            Method::PATCH,
                            &format!("{CATEGORIES}/{}", path_segment(id)),
                            Some(&json!({ "color": settings.color.as_str() })),
                            &[],
                            true,
                            "PATCH masterCategories",
                        )
                        .await?;
                }
                Ok(None)
            }
            Some(_) => {
                let new_id = match self.category_named(&settings.name).await? {
                    Some(existing) => {
                        let mut s = settings.clone();
                        s.name.clone_from(&existing.name);
                        Box::pin(self.change_category(&existing.id, &s)).await?;
                        existing.id
                    }
                    None => self.add_category(settings).await?,
                };
                self.remove_category(id).await?;
                Ok(Some(new_id))
            }
            // Deleted elsewhere: a replayed rename after it already went
            // through, or a category that is gone.
            None => match self.category_named(&settings.name).await? {
                Some(existing) => Ok(Some(existing.id)),
                None => Err(GraphError::NotFound(format!("category {id}"))),
            },
        }
    }

    /// Deletes a category (an unknown id is not an error).
    pub async fn remove_category(&self, id: &str) -> Result<(), GraphError> {
        match self
            .client
            .send_json(
                Method::DELETE,
                &format!("{CATEGORIES}/{}", path_segment(id)),
                None,
                &[],
                true,
                "DELETE masterCategories",
            )
            .await
        {
            Ok(_) | Err(GraphError::NotFound(_)) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// The automatic replies of a mailbox (`None` = own), replies as text.
    pub async fn read_automatic_replies(
        &self,
        mailbox: Option<&str>,
    ) -> Result<AutomaticRepliesSettings, GraphError> {
        let setting: AutomaticRepliesSetting = self
            .client
            .get_json(
                &format!(
                    "{}/mailboxSettings/automaticRepliesSetting",
                    mailbox_path(mailbox)
                ),
                &[],
                &[("Prefer", "outlook.timezone=\"UTC\"".to_owned())],
                "GET automaticRepliesSetting",
            )
            .await?;
        Ok(automatic_replies_from_graph(&setting))
    }

    /// Replaces the automatic replies (replies written as simple HTML).
    pub async fn write_automatic_replies(
        &self,
        mailbox: Option<&str>,
        settings: &AutomaticRepliesSettings,
    ) -> Result<(), GraphError> {
        self.client
            .send_json(
                Method::PATCH,
                &format!("{}/mailboxSettings", mailbox_path(mailbox)),
                Some(&json!({ "automaticRepliesSetting": automatic_replies_to_graph(settings) })),
                &[],
                true,
                "PATCH mailboxSettings",
            )
            .await
            .map(drop)
    }

    /// Every inbox rule, by sequence.
    pub async fn list_rules(&self) -> Result<Vec<ExchangeInboxRule>, GraphError> {
        let page: Page<Value> = self
            .client
            .get_json(RULES, &[], &[], "GET messageRules")
            .await?;
        let mut rules: Vec<ExchangeInboxRule> = page.value.iter().map(rule_from_graph).collect();
        rules.sort_by_key(|r| r.sequence);
        Ok(rules)
    }

    /// Creates an inbox rule; answers its id.
    pub async fn add_rule(&self, rule: &ExchangeInboxRule) -> Result<String, GraphError> {
        let created = self
            .client
            .send_json(
                Method::POST,
                RULES,
                Some(&rule_to_graph(rule)),
                &[],
                false,
                "POST messageRules",
            )
            .await?;
        created
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| GraphError::Protocol("messageRules answered no id".into()))
    }

    /// Replaces an inbox rule; a rule with unsupported parts only changes
    /// `isEnabled`.
    pub async fn change_rule(&self, rule: &ExchangeInboxRule) -> Result<(), GraphError> {
        let body = if rule.has_unsupported {
            json!({ "isEnabled": rule.enabled })
        } else {
            rule_to_graph(rule)
        };
        self.client
            .send_json(
                Method::PATCH,
                &format!("{RULES}/{}", path_segment(&rule.id)),
                Some(&body),
                &[],
                true,
                "PATCH messageRules",
            )
            .await
            .map(drop)
    }

    /// Deletes an inbox rule (an unknown id is not an error).
    pub async fn remove_rule(&self, id: &str) -> Result<(), GraphError> {
        match self
            .client
            .send_json(
                Method::DELETE,
                &format!("{RULES}/{}", path_segment(id)),
                None,
                &[],
                true,
                "DELETE messageRules",
            )
            .await
        {
            Ok(_) | Err(GraphError::NotFound(_)) => Ok(()),
            Err(e) => Err(e),
        }
    }
}

fn utc_of(time: Option<&DateTimeTimeZone>) -> Option<String> {
    let time = time?;
    if time.date_time.is_empty() {
        return None;
    }
    let zone = time.time_zone.as_deref().unwrap_or("UTC");
    if !zone.eq_ignore_ascii_case("UTC") {
        // Asked for UTC with `Prefer: outlook.timezone`; a server that
        // still answers in another zone is read as UTC.
        tracing::warn!(zone, "automatic replies schedule not in UTC");
    }
    text::to_utc(&time.date_time)
}

/// Graph's automatic replies as the contract's settings.
pub fn automatic_replies_from_graph(setting: &AutomaticRepliesSetting) -> AutomaticRepliesSettings {
    let status = match setting.status.as_deref() {
        Some("alwaysEnabled") => AutomaticRepliesStatus::AlwaysEnabled,
        Some("scheduled") => AutomaticRepliesStatus::Scheduled,
        _ => AutomaticRepliesStatus::Disabled,
    };
    let external_audience = match setting.external_audience.as_deref() {
        Some("none") => ExternalAudience::None,
        Some("contactsOnly") => ExternalAudience::ContactsOnly,
        _ => ExternalAudience::All,
    };
    AutomaticRepliesSettings {
        status,
        start: utc_of(setting.scheduled_start_date_time.as_ref()),
        end: utc_of(setting.scheduled_end_date_time.as_ref()),
        internal_reply: text::html_to_text(setting.internal_reply_message.as_deref().unwrap_or("")),
        external_reply: text::html_to_text(setting.external_reply_message.as_deref().unwrap_or("")),
        external_audience,
    }
}

/// The contract's settings as Graph's `automaticRepliesSetting`.
pub fn automatic_replies_to_graph(settings: &AutomaticRepliesSettings) -> Value {
    let status = match settings.status {
        AutomaticRepliesStatus::Disabled => "disabled",
        AutomaticRepliesStatus::AlwaysEnabled => "alwaysEnabled",
        AutomaticRepliesStatus::Scheduled => "scheduled",
    };
    let audience = match settings.external_audience {
        ExternalAudience::None => "none",
        ExternalAudience::ContactsOnly => "contactsOnly",
        ExternalAudience::All => "all",
    };
    let mut out = json!({
        "status": status,
        "externalAudience": audience,
        "internalReplyMessage": text::text_to_html(&settings.internal_reply),
        "externalReplyMessage": text::text_to_html(&settings.external_reply),
    });
    for (key, time) in [
        ("scheduledStartDateTime", &settings.start),
        ("scheduledEndDateTime", &settings.end),
    ] {
        if let Some(time) = time {
            out[key] = json!({
                "dateTime": text::trim_fraction(time),
                "timeZone": "UTC",
            });
        }
    }
    out
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn recipients(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|r| r.pointer("/emailAddress/address").and_then(Value::as_str))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn importance(value: Option<&Value>) -> Option<Importance> {
    match value.and_then(Value::as_str)? {
        "low" => Some(Importance::Low),
        "normal" => Some(Importance::Normal),
        "high" => Some(Importance::High),
        _ => None,
    }
}

fn importance_name(importance: Importance) -> &'static str {
    match importance {
        Importance::Low => "low",
        Importance::Normal => "normal",
        Importance::High => "high",
    }
}

/// Whether a JSON value counts as "set" (not null / false / empty).
fn is_set(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(false) => false,
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => o.values().any(is_set),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

fn has_unknown(object: Option<&Value>, known: &[&str]) -> bool {
    object.and_then(Value::as_object).is_some_and(|o| {
        o.iter()
            .any(|(k, v)| !known.contains(&k.as_str()) && is_set(v))
    })
}

fn conditions_from_graph(value: Option<&Value>) -> ExchangeRuleConditions {
    let Some(v) = value else {
        return ExchangeRuleConditions::default();
    };
    let flag = |name: &str| v.get(name).and_then(Value::as_bool).filter(|b| *b);
    let size = v.get("withinSizeRange");
    ExchangeRuleConditions {
        from_addresses: recipients(v.get("fromAddresses")),
        sent_to_addresses: recipients(v.get("sentToAddresses")),
        subject_contains: strings(v.get("subjectContains")),
        body_contains: strings(v.get("bodyContains")),
        body_or_subject_contains: strings(v.get("bodyOrSubjectContains")),
        header_contains: strings(v.get("headerContains")),
        sender_contains: strings(v.get("senderContains")),
        recipient_contains: strings(v.get("recipientContains")),
        categories: strings(v.get("categories")),
        has_attachments: flag("hasAttachments"),
        importance: importance(v.get("importance")),
        sent_to_me: flag("sentToMe"),
        sent_only_to_me: flag("sentOnlyToMe"),
        sent_cc_me: flag("sentCcMe"),
        sent_to_or_cc_me: flag("sentToOrCcMe"),
        not_sent_to_me: flag("notSentToMe"),
        is_meeting_request: flag("isMeetingRequest"),
        min_size_kb: size
            .and_then(|s| s.get("minimumSize"))
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok()),
        max_size_kb: size
            .and_then(|s| s.get("maximumSize"))
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok()),
    }
}

fn conditions_to_graph(c: &ExchangeRuleConditions) -> Value {
    let mut out = Map::new();
    let mut list = |name: &str, values: &[String]| {
        if !values.is_empty() {
            out.insert(name.to_owned(), json!(values));
        }
    };
    list("subjectContains", &c.subject_contains);
    list("bodyContains", &c.body_contains);
    list("bodyOrSubjectContains", &c.body_or_subject_contains);
    list("headerContains", &c.header_contains);
    list("senderContains", &c.sender_contains);
    list("recipientContains", &c.recipient_contains);
    list("categories", &c.categories);
    for (name, addresses) in [
        ("fromAddresses", &c.from_addresses),
        ("sentToAddresses", &c.sent_to_addresses),
    ] {
        if !addresses.is_empty() {
            out.insert(name.to_owned(), recipient_list(addresses));
        }
    }
    for (name, value) in [
        ("hasAttachments", c.has_attachments),
        ("sentToMe", c.sent_to_me),
        ("sentOnlyToMe", c.sent_only_to_me),
        ("sentCcMe", c.sent_cc_me),
        ("sentToOrCcMe", c.sent_to_or_cc_me),
        ("notSentToMe", c.not_sent_to_me),
        ("isMeetingRequest", c.is_meeting_request),
    ] {
        if let Some(value) = value {
            out.insert(name.to_owned(), json!(value));
        }
    }
    if let Some(i) = c.importance {
        out.insert("importance".into(), json!(importance_name(i)));
    }
    if c.min_size_kb.is_some() || c.max_size_kb.is_some() {
        let mut range = Map::new();
        if let Some(min) = c.min_size_kb {
            range.insert("minimumSize".into(), json!(min));
        }
        if let Some(max) = c.max_size_kb {
            range.insert("maximumSize".into(), json!(max));
        }
        out.insert("withinSizeRange".into(), Value::Object(range));
    }
    Value::Object(out)
}

fn recipient_list(addresses: &[String]) -> Value {
    Value::Array(
        addresses
            .iter()
            .map(|a| json!({ "emailAddress": { "address": a } }))
            .collect(),
    )
}

fn actions_from_graph(value: Option<&Value>) -> ExchangeRuleActions {
    let Some(v) = value else {
        return ExchangeRuleActions::default();
    };
    let flag = |name: &str| v.get(name).and_then(Value::as_bool).unwrap_or(false);
    let folder = |name: &str| {
        v.get(name)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(FolderId::new)
    };
    ExchangeRuleActions {
        move_to_folder: folder("moveToFolder"),
        copy_to_folder: folder("copyToFolder"),
        delete: flag("delete"),
        permanent_delete: flag("permanentDelete"),
        mark_as_read: flag("markAsRead"),
        mark_importance: importance(v.get("markImportance")),
        assign_categories: strings(v.get("assignCategories")),
        forward_to: recipients(v.get("forwardTo")),
        forward_as_attachment_to: recipients(v.get("forwardAsAttachmentTo")),
        redirect_to: recipients(v.get("redirectTo")),
        stop_processing_rules: flag("stopProcessingRules"),
    }
}

fn actions_to_graph(a: &ExchangeRuleActions) -> Value {
    let mut out = Map::new();
    if let Some(f) = &a.move_to_folder {
        out.insert("moveToFolder".into(), json!(f.as_str()));
    }
    if let Some(f) = &a.copy_to_folder {
        out.insert("copyToFolder".into(), json!(f.as_str()));
    }
    for (name, value) in [
        ("delete", a.delete),
        ("permanentDelete", a.permanent_delete),
        ("markAsRead", a.mark_as_read),
        ("stopProcessingRules", a.stop_processing_rules),
    ] {
        if value {
            out.insert(name.to_owned(), json!(true));
        }
    }
    if let Some(i) = a.mark_importance {
        out.insert("markImportance".into(), json!(importance_name(i)));
    }
    if !a.assign_categories.is_empty() {
        out.insert("assignCategories".into(), json!(a.assign_categories));
    }
    for (name, list) in [
        ("forwardTo", &a.forward_to),
        ("forwardAsAttachmentTo", &a.forward_as_attachment_to),
        ("redirectTo", &a.redirect_to),
    ] {
        if !list.is_empty() {
            out.insert(name.to_owned(), recipient_list(list));
        }
    }
    Value::Object(out)
}

/// A Graph `messageRule` as the contract's rule. `move_to_folder` /
/// `copy_to_folder` carry Graph folder ids (the core maps them to its
/// folders).
pub fn rule_from_graph(value: &Value) -> ExchangeInboxRule {
    let conditions = value.get("conditions");
    let exceptions = value.get("exceptions");
    let actions = value.get("actions");
    let has_unsupported = has_unknown(conditions, &KNOWN_PREDICATES)
        || has_unknown(exceptions, &KNOWN_PREDICATES)
        || has_unknown(actions, &KNOWN_ACTIONS)
        || value.get("isReadOnly").and_then(Value::as_bool) == Some(true);
    ExchangeInboxRule {
        id: value
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        name: value
            .get("displayName")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        sequence: value
            .get("sequence")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(0),
        enabled: value
            .get("isEnabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        conditions: conditions_from_graph(conditions),
        exceptions: conditions_from_graph(exceptions),
        actions: actions_from_graph(actions),
        has_unsupported,
        in_error: value.get("hasError").and_then(Value::as_bool) == Some(true),
        nail_rule_id: None,
    }
}

/// The contract's rule as a Graph `messageRule` (without the id).
pub fn rule_to_graph(rule: &ExchangeInboxRule) -> Value {
    let mut out = json!({
        "displayName": rule.name,
        "sequence": rule.sequence.max(1),
        "isEnabled": rule.enabled,
        "conditions": conditions_to_graph(&rule.conditions),
        "actions": actions_to_graph(&rule.actions),
    });
    if !rule.exceptions.is_empty() {
        out["exceptions"] = conditions_to_graph(&rule.exceptions);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn recorded_rule() -> Value {
        serde_json::from_str(include_str!("../fixtures/responses/message-rule.json")).unwrap()
    }

    #[test]
    fn recorded_rule_reads_and_round_trips() {
        let rule = rule_from_graph(&recorded_rule());
        assert_eq!(rule.name, "From partner");
        assert_eq!(rule.sequence, 2);
        assert!(rule.enabled && !rule.in_error && !rule.has_unsupported);
        assert_eq!(rule.conditions.sender_contains, ["adele"]);
        assert_eq!(rule.conditions.from_addresses, ["partner@fabrikam.example"]);
        assert_eq!(rule.conditions.min_size_kb, Some(10));
        assert_eq!(rule.exceptions.subject_contains, ["Urgent"]);
        assert_eq!(
            rule.actions.move_to_folder,
            Some(FolderId::new("AAMkPartners"))
        );
        assert_eq!(rule.actions.assign_categories, ["Partner"]);
        assert_eq!(rule.actions.forward_to, ["ada@contoso.example"]);
        assert!(rule.actions.stop_processing_rules);
        let back = rule_from_graph(&rule_to_graph(&rule));
        assert_eq!(
            ExchangeInboxRule {
                id: rule.id.clone(),
                ..back
            },
            rule
        );
    }

    #[test]
    fn unknown_parts_make_rules_read_only() {
        let mut value = recorded_rule();
        value["conditions"]["isAutomaticReply"] = json!(true);
        assert!(rule_from_graph(&value).has_unsupported);
        let mut value = recorded_rule();
        value["conditions"]["isAutomaticReply"] = json!(false);
        value["conditions"]["sensitivity"] = Value::Null;
        assert!(
            !rule_from_graph(&value).has_unsupported,
            "unset parts are fine"
        );
        let mut value = recorded_rule();
        value["actions"]["permanentDelete"] = json!(false);
        value["actions"]["markAsRead"] = json!(true);
        assert!(!rule_from_graph(&value).has_unsupported);
        value["isReadOnly"] = json!(true);
        assert!(rule_from_graph(&value).has_unsupported);
        let mut value = recorded_rule();
        value["hasError"] = json!(true);
        value["exceptions"]["messageActionFlag"] = json!("reply");
        let rule = rule_from_graph(&value);
        assert!(rule.in_error && rule.has_unsupported);
        let empty = rule_from_graph(&json!({"id": "r"}));
        assert!(empty.conditions.is_empty() && empty.actions.is_empty() && !empty.enabled);
    }

    #[test]
    fn rules_are_written_with_set_parts_only() {
        let rule = ExchangeInboxRule {
            id: "x".into(),
            name: "Newsletters".into(),
            sequence: 0,
            enabled: true,
            conditions: ExchangeRuleConditions {
                sender_contains: vec!["@substack.example".into()],
                importance: Some(Importance::Low),
                ..ExchangeRuleConditions::default()
            },
            exceptions: ExchangeRuleConditions::default(),
            actions: ExchangeRuleActions {
                mark_as_read: true,
                mark_importance: Some(Importance::High),
                redirect_to: vec!["bob@contoso.example".into()],
                ..ExchangeRuleActions::default()
            },
            has_unsupported: false,
            in_error: false,
            nail_rule_id: None,
        };
        assert_eq!(
            rule_to_graph(&rule),
            json!({
                "displayName": "Newsletters", "sequence": 1, "isEnabled": true,
                "conditions": {"senderContains": ["@substack.example"], "importance": "low"},
                "actions": {"markAsRead": true, "markImportance": "high",
                            "redirectTo": [{"emailAddress": {"address": "bob@contoso.example"}}]}
            })
        );
    }

    #[test]
    fn automatic_replies_convert_both_ways() {
        let recorded: AutomaticRepliesSetting =
            serde_json::from_str(include_str!("../fixtures/responses/automatic-replies.json"))
                .unwrap();
        let settings = automatic_replies_from_graph(&recorded);
        assert_eq!(settings.status, AutomaticRepliesStatus::Scheduled);
        assert_eq!(settings.start.as_deref(), Some("2026-10-05T07:00:00Z"));
        assert_eq!(settings.end.as_deref(), Some("2026-10-09T16:00:00Z"));
        assert_eq!(settings.external_audience, ExternalAudience::ContactsOnly);
        assert_eq!(
            settings.internal_reply,
            "I am out of the office until Friday.\nAsk Bob for urgent matters."
        );
        assert_eq!(
            settings.external_reply,
            "Thanks for your mail. I will answer next week."
        );
        let graph = automatic_replies_to_graph(&settings);
        assert_eq!(graph["status"], "scheduled");
        assert_eq!(graph["externalAudience"], "contactsOnly");
        assert_eq!(
            graph["scheduledStartDateTime"],
            json!({"dateTime": "2026-10-05T07:00:00", "timeZone": "UTC"})
        );
        let back: AutomaticRepliesSetting = serde_json::from_value(graph).unwrap();
        assert_eq!(automatic_replies_from_graph(&back), settings);
        let off = automatic_replies_to_graph(&AutomaticRepliesSettings::default());
        assert!(off.get("scheduledStartDateTime").is_none());
        assert_eq!(off["status"], "disabled");
        assert_eq!(
            automatic_replies_from_graph(&AutomaticRepliesSetting::default()),
            AutomaticRepliesSettings::default()
        );
    }

    #[test]
    fn categories_map_colours() {
        let c = category(OutlookCategory {
            id: "1".into(),
            display_name: "Red category".into(),
            color: Some("preset0".into()),
        });
        assert_eq!(c.color, CategoryColor::Preset0);
        let none = category(OutlookCategory {
            id: "2".into(),
            display_name: "x".into(),
            color: Some("bogus".into()),
        });
        assert_eq!(none.color, CategoryColor::None);
    }
}
