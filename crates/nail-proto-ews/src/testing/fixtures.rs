//! The mock server's default organization and recorded-style EWS answers.
//!
//! The recorded answers are shaped like Exchange Server 2019's (CU14):
//! namespaces, prefixes, `ServerVersionInfo`, the order of elements and
//! the error details; ids and addresses are synthetic (`corp.example`).
//! Tests replay them with [`super::MockEwsServer::answer_with`] or parse
//! them directly.

use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};

use super::model::{Access, MockFolder, MockMailbox, Oof, State};
use super::{
    DELEGATE_ADDRESS, MockAuth, OWN_ADDRESS, PRIVATE_ADDRESS, ROOM_LIST, ROOMS, SHARED_ADDRESS,
};
use crate::types::format_utc;
use crate::xml::{self, Element};

/// The demo messages of the own inbox (raw RFC 5322).
pub const INBOX_MESSAGES: [&str; 3] = [
    concat!(
        "From: Bob Builder <bob@corp.example>\r\n",
        "To: Ada Lovelace <ada@corp.example>\r\n",
        "Subject: Quarterly numbers\r\n",
        "Date: Tue, 29 Sep 2026 09:15:00 +0200\r\n",
        "Message-ID: <q3-numbers@corp.example>\r\n",
        "MIME-Version: 1.0\r\n",
        "Content-Type: text/plain; charset=utf-8\r\n",
        "\r\n",
        "Hi Ada,\r\n\r\nthe Q3 numbers are in the shared folder.\r\n\r\nBob\r\n"
    ),
    concat!(
        "From: Carol Shaw <carol@corp.example>\r\n",
        "To: Ada Lovelace <ada@corp.example>\r\n",
        "Subject: Lunch on Friday?\r\n",
        "Date: Wed, 30 Sep 2026 11:02:00 +0200\r\n",
        "Message-ID: <lunch-friday@corp.example>\r\n",
        "MIME-Version: 1.0\r\n",
        "Content-Type: text/plain; charset=utf-8\r\n",
        "\r\n",
        "Shall we try the new place?\r\n"
    ),
    concat!(
        "From: IT Service Desk <it@corp.example>\r\n",
        "To: Ada Lovelace <ada@corp.example>\r\n",
        "Subject: Password expires in 7 days\r\n",
        "Date: Thu, 01 Oct 2026 07:30:00 +0200\r\n",
        "Message-ID: <pw-expiry@corp.example>\r\n",
        "MIME-Version: 1.0\r\n",
        "Content-Type: text/plain; charset=utf-8\r\n",
        "\r\n",
        "Change it at https://password.corp.example before it expires.\r\n"
    ),
];

/// A receipt in `Inbox/Receipts`.
pub const RECEIPT: &str = concat!(
    "From: Shop <orders@shop.example>\r\n",
    "To: ada@corp.example\r\n",
    "Subject: Your order 4711\r\n",
    "Date: Mon, 28 Sep 2026 18:00:00 +0200\r\n",
    "Message-ID: <order-4711@shop.example>\r\n",
    "\r\n",
    "Thank you for your order.\r\n"
);

/// A message in the shared mailbox.
pub const SHARED_MESSAGE: &str = concat!(
    "From: Customer <customer@client.example>\r\n",
    "To: Support <shared@corp.example>\r\n",
    "Subject: Ticket 1234: printer offline\r\n",
    "Date: Wed, 30 Sep 2026 16:45:00 +0200\r\n",
    "Message-ID: <ticket-1234@client.example>\r\n",
    "\r\n",
    "The printer on floor 3 is offline again.\r\n"
);

/// The category list of the own mailbox.
pub const CATEGORY_LIST: &str = concat!(
    r#"<?xml version="1.0" encoding="utf-8"?>"#,
    r#"<categories default="Red Category" lastSavedSession="1" lastSavedTime="2026-09-01T10:00:00.000" xmlns="CategoryList.xsd">"#,
    r#"<category name="Red Category" color="0" keyboardShortcut="0" usageCount="0" lastTimeUsedNotes="1601-01-01T00:00:00.000" "#,
    r#"guid="{98a1a2c3-0000-4000-8000-000000000001}" renameOnFirstUse="1"/>"#,
    r#"<category name="Work" color="7" keyboardShortcut="0" usageCount="5" "#,
    r#"guid="{98a1a2c3-0000-4000-8000-000000000002}" renameOnFirstUse="0"/>"#,
    r#"<category name="Personal" color="4" keyboardShortcut="0" usageCount="1" "#,
    r#"guid="{98a1a2c3-0000-4000-8000-000000000003}" renameOnFirstUse="0"/>"#,
    "</categories>"
);

/// An inbox rule Nail can express.
pub const RULE_NEWSLETTERS: &str = concat!(
    "<Rule><DisplayName>Newsletters</DisplayName><Priority>1</Priority><IsEnabled>true</IsEnabled>",
    "<Conditions><ContainsSenderStrings><String>@news.example</String></ContainsSenderStrings></Conditions>",
    "<Actions><AssignCategories><String>Personal</String></AssignCategories><MarkAsRead>true</MarkAsRead></Actions>",
    "<IsNotSupported>false</IsNotSupported><IsInError>false</IsInError></Rule>"
);

/// An inbox rule with a condition outside Nail's subset.
pub const RULE_VOICEMAIL: &str = concat!(
    "<Rule><DisplayName>Voicemail</DisplayName><Priority>2</Priority><IsEnabled>true</IsEnabled>",
    "<Conditions><IsVoicemail>true</IsVoicemail></Conditions>",
    "<Actions><MarkImportance>High</MarkImportance></Actions>",
    "<IsNotSupported>false</IsNotSupported><IsInError>false</IsInError></Rule>"
);

fn conversation(subject: &str) -> String {
    let topic = subject
        .trim_start_matches("Re: ")
        .trim_start_matches("RE: ")
        .trim_start_matches("Fwd: ")
        .to_ascii_lowercase();
    let mut hasher = DefaultHasher::new();
    topic.hash(&mut hasher);
    format!("AAQkConv{:016x}", hasher.finish())
}

fn header<'a>(raw: &'a str, name: &str) -> Option<&'a str> {
    let head = raw.split("\r\n\r\n").next().unwrap_or(raw);
    head.split("\r\n").find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim().eq_ignore_ascii_case(name).then(|| value.trim())
    })
}

fn child(name: &str, text: &str) -> Element {
    let mut e = Element::new(name);
    text.clone_into(&mut e.text);
    e
}

/// The properties of a message item made from raw MIME.
pub fn message_props(raw: &[u8], read: bool, categories: &[String]) -> Element {
    let text = String::from_utf8_lossy(raw);
    let subject = header(&text, "Subject").unwrap_or_default().to_owned();
    let received = header(&text, "Date")
        .and_then(|d| chrono::DateTime::parse_from_rfc2822(d).ok())
        .map_or_else(chrono::Utc::now, |d| d.with_timezone(&chrono::Utc));
    let mut props = Element::new("Message");
    props.children.push(child("ItemClass", "IPM.Note"));
    props.children.push(child("Subject", &subject));
    props
        .children
        .push(child("DateTimeReceived", &format_utc(received)));
    props.children.push(child("Size", &raw.len().to_string()));
    if !categories.is_empty() {
        let mut list = Element::new("Categories");
        for c in categories {
            list.children.push(child("String", c));
        }
        props.children.push(list);
    }
    props.children.push(child("IsDraft", "false"));
    let mut conversation_id = Element::new("ConversationId");
    conversation_id.set_attr("Id", conversation(&subject));
    props.children.push(conversation_id);
    if let Some(id) = header(&text, "Message-ID") {
        props.children.push(child("InternetMessageId", id));
    }
    props
        .children
        .push(child("IsRead", if read { "true" } else { "false" }));
    props
}

struct Builder {
    state: State,
}

impl Builder {
    fn mailbox(&mut self, address: &str, name: &str, access: Access) -> usize {
        let root = self.state.new_id("AQMkRoot");
        let key = self.state.new_key();
        self.state.mailboxes.push(MockMailbox {
            address: address.to_owned(),
            name: name.to_owned(),
            access,
            folders: vec![MockFolder {
                id: root,
                change_key: key,
                parent: None,
                name: "Top of Information Store".into(),
                class: "IPF.Note".into(),
                distinguished: Some("msgfolderroot".into()),
            }],
            items: Vec::new(),
        });
        self.state.mailboxes.len() - 1
    }

    fn folder(
        &mut self,
        m: usize,
        parent: Option<&str>,
        name: &str,
        class: &str,
        distinguished: Option<&str>,
    ) -> String {
        let id = self.state.new_id("AQMkFolder");
        let key = self.state.new_key();
        let parent = parent.map_or_else(
            || self.state.mailboxes[m].folders[0].id.clone(),
            str::to_owned,
        );
        self.state.mailboxes[m].folders.push(MockFolder {
            id: id.clone(),
            change_key: key,
            parent: Some(parent),
            name: name.to_owned(),
            class: class.to_owned(),
            distinguished: distinguished.map(str::to_owned),
        });
        id
    }

    fn standard_folders(&mut self, m: usize) -> HashMap<&'static str, String> {
        let mut out = HashMap::new();
        for (name, class, id) in [
            ("Inbox", "IPF.Note", "inbox"),
            ("Sent Items", "IPF.Note", "sentitems"),
            ("Drafts", "IPF.Note", "drafts"),
            ("Deleted Items", "IPF.Note", "deleteditems"),
            ("Junk Email", "IPF.Note", "junkemail"),
            ("Outbox", "IPF.Note", "outbox"),
            ("Calendar", "IPF.Appointment", "calendar"),
            ("Contacts", "IPF.Contact", "contacts"),
            ("Tasks", "IPF.Task", "tasks"),
        ] {
            let folder = self.folder(m, None, name, class, Some(id));
            out.insert(id, folder);
        }
        out
    }

    fn message(
        &mut self,
        m: usize,
        folder: &str,
        raw: &str,
        read: bool,
        categories: &[&str],
    ) -> String {
        let categories: Vec<String> = categories.iter().map(|c| (*c).to_owned()).collect();
        let props = message_props(raw.as_bytes(), read, &categories);
        self.state
            .add_item(m, folder, "Message", props, Some(raw.as_bytes().to_vec()))
    }

    fn pim(&mut self, m: usize, folder: &str, tag: &str, props: &str) -> String {
        let props = xml::parse(&format!("<{tag}>{props}</{tag}>")).unwrap_or_default();
        self.state.add_item(m, folder, tag, props, None)
    }
}

/// The default organization: Ada's mailbox (inbox with a sub-folder,
/// calendar with a weekly series, contacts, tasks), the shared mailbox
/// `shared@`, Grace's mailbox `boss@` (calendar, contacts and tasks open
/// to Ada as a delegate), `private@` (closed), Bob and Carol in the
/// directory, one room list with two rooms, categories, two inbox rules
/// and Bob as Ada's delegate.
pub fn organization() -> State {
    let mut b = Builder {
        state: State {
            own: OWN_ADDRESS.to_owned(),
            mailboxes: Vec::new(),
            next_id: 0,
            sync_states: HashMap::new(),
            hierarchy_states: HashMap::new(),
            subscriptions: HashMap::new(),
            log: Vec::new(),
            faults: Vec::new(),
            recorded: Vec::new(),
            auth: MockAuth::None,
            authenticated: HashSet::new(),
            challenged: HashSet::new(),
            sent: Vec::new(),
            meeting_messages: Vec::new(),
            oof: Oof::default(),
            rules: Vec::new(),
            outlook_rule_blob: false,
            categories: Some(CATEGORY_LIST.to_owned()),
            room_lists: vec![(
                ROOM_LIST.to_owned(),
                "Headquarters".to_owned(),
                vec![
                    (ROOMS[0].to_owned(), "Room 1 (8 seats)".to_owned()),
                    (ROOMS[1].to_owned(), "Room 2 (20 seats)".to_owned()),
                ],
            )],
            rooms_denied: false,
            directory: vec![
                ("bob@corp.example".to_owned(), "Bob Builder".to_owned()),
                ("carol@corp.example".to_owned(), "Carol Shaw".to_owned()),
            ],
            free_busy: HashMap::new(),
            delegates: vec![(
                "bob@corp.example".to_owned(),
                "Bob Builder".to_owned(),
                "Editor".to_owned(),
            )],
            refused_versions: Vec::new(),
            version: (15, 2, 1544, 4),
            stream_close_after: std::time::Duration::from_secs(30),
            break_streams: 0,
            cross_mailbox_moves: true,
        },
    };
    let own = b.mailbox(OWN_ADDRESS, "Ada Lovelace", Access::Full);
    let folders = b.standard_folders(own);
    let inbox = folders["inbox"].clone();
    b.folder(own, None, "Archive", "IPF.Note", None);
    b.folder(own, None, "Notes", "IPF.StickyNote", None);
    let receipts = b.folder(own, Some(&inbox), "Receipts", "IPF.Note", None);
    b.message(own, &inbox, INBOX_MESSAGES[0], true, &["Work"]);
    b.message(own, &inbox, INBOX_MESSAGES[1], false, &[]);
    b.message(own, &inbox, INBOX_MESSAGES[2], false, &[]);
    b.message(own, &receipts, RECEIPT, true, &[]);
    let calendar = folders["calendar"].clone();
    b.pim(
        own,
        &calendar,
        "CalendarItem",
        concat!(
            "<Subject>Team sync</Subject><Body BodyType=\"Text\">Weekly round</Body>",
            "<Sensitivity>Normal</Sensitivity><ReminderIsSet>true</ReminderIsSet>",
            "<ReminderMinutesBeforeStart>15</ReminderMinutesBeforeStart>",
            "<UID>040000008200E00074C5B7101A82E0080000000001</UID>",
            "<Start>2026-10-05T07:00:00Z</Start><End>2026-10-05T07:30:00Z</End>",
            "<IsAllDayEvent>false</IsAllDayEvent><LegacyFreeBusyStatus>Busy</LegacyFreeBusyStatus>",
            "<Location>Room 1 (8 seats)</Location><IsCancelled>false</IsCancelled>",
            "<IsResponseRequested>true</IsResponseRequested><CalendarItemType>RecurringMaster</CalendarItemType>",
            "<MyResponseType>Organizer</MyResponseType>",
            "<Organizer><Mailbox><Name>Ada Lovelace</Name><EmailAddress>ada@corp.example</EmailAddress></Mailbox></Organizer>",
            "<RequiredAttendees><Attendee><Mailbox><Name>Bob Builder</Name><EmailAddress>bob@corp.example</EmailAddress></Mailbox>",
            "<ResponseType>Accept</ResponseType></Attendee></RequiredAttendees>",
            "<Recurrence><WeeklyRecurrence><Interval>1</Interval><DaysOfWeek>Monday</DaysOfWeek>",
            "<FirstDayOfWeek>Monday</FirstDayOfWeek></WeeklyRecurrence>",
            "<NumberedRecurrence><StartDate>2026-10-05</StartDate><NumberOfOccurrences>10</NumberOfOccurrences></NumberedRecurrence></Recurrence>",
            "<StartTimeZone Id=\"W. Europe Standard Time\"/><IsOrganizer>true</IsOrganizer>"
        ),
    );
    b.pim(
        own,
        &calendar,
        "CalendarItem",
        concat!(
            "<Subject>Board review</Subject><Sensitivity>Normal</Sensitivity>",
            "<UID>040000008200E00074C5B7101A82E0080000000002</UID>",
            "<Start>2026-10-07T12:00:00Z</Start><End>2026-10-07T13:00:00Z</End>",
            "<IsAllDayEvent>false</IsAllDayEvent><LegacyFreeBusyStatus>Tentative</LegacyFreeBusyStatus>",
            "<IsCancelled>false</IsCancelled><IsResponseRequested>true</IsResponseRequested>",
            "<CalendarItemType>Single</CalendarItemType><MyResponseType>NoResponseReceived</MyResponseType>",
            "<Organizer><Mailbox><Name>Grace Hopper</Name><EmailAddress>boss@corp.example</EmailAddress></Mailbox></Organizer>",
            "<RequiredAttendees><Attendee><Mailbox><Name>Ada Lovelace</Name><EmailAddress>ada@corp.example</EmailAddress></Mailbox>",
            "<ResponseType>NoResponseReceived</ResponseType></Attendee></RequiredAttendees>",
            "<StartTimeZone Id=\"W. Europe Standard Time\"/><IsOrganizer>false</IsOrganizer>"
        ),
    );
    let contacts = folders["contacts"].clone();
    b.pim(
        own,
        &contacts,
        "Contact",
        concat!(
            "<FileAs>Builder, Bob</FileAs><DisplayName>Bob Builder</DisplayName><GivenName>Bob</GivenName>",
            "<CompanyName>Corp</CompanyName><EmailAddresses><Entry Key=\"EmailAddress1\">bob@corp.example</Entry></EmailAddresses>",
            "<PhoneNumbers><Entry Key=\"MobilePhone\">+49 170 0000000</Entry></PhoneNumbers>",
            "<JobTitle>Builder</JobTitle><Surname>Builder</Surname>"
        ),
    );
    b.pim(
        own,
        &contacts,
        "DistributionList",
        concat!(
            "<FileAs>Project team</FileAs><DisplayName>Project team</DisplayName><Members>",
            "<Member><Mailbox><Name>Bob Builder</Name><EmailAddress>bob@corp.example</EmailAddress></Mailbox></Member>",
            "<Member><Mailbox><Name>Carol Shaw</Name><EmailAddress>carol@corp.example</EmailAddress></Mailbox></Member>",
            "</Members>"
        ),
    );
    let tasks = folders["tasks"].clone();
    b.pim(
        own,
        &tasks,
        "Task",
        concat!(
            "<Subject>Submit expense report</Subject><Body BodyType=\"Text\">September trip</Body>",
            "<Importance>High</Importance><DueDate>2026-10-09T22:00:00Z</DueDate>",
            "<Status>NotStarted</Status><IsComplete>false</IsComplete>"
        ),
    );

    let shared = b.mailbox(SHARED_ADDRESS, "Support", Access::Full);
    for (name, class, id) in [
        ("Inbox", "IPF.Note", "inbox"),
        ("Sent Items", "IPF.Note", "sentitems"),
        ("Drafts", "IPF.Note", "drafts"),
        ("Deleted Items", "IPF.Note", "deleteditems"),
        ("Calendar", "IPF.Appointment", "calendar"),
    ] {
        b.folder(shared, None, name, class, Some(id));
    }
    let shared_inbox = b.state.mailboxes[shared].folders[1].id.clone();
    b.message(shared, &shared_inbox, SHARED_MESSAGE, false, &[]);

    let boss = b.mailbox(DELEGATE_ADDRESS, "Grace Hopper", Access::PimOnly);
    let boss_folders = b.standard_folders(boss);
    b.pim(
        boss,
        &boss_folders["calendar"].clone(),
        "CalendarItem",
        concat!(
            "<Subject>Budget planning</Subject><Sensitivity>Normal</Sensitivity>",
            "<UID>040000008200E00074C5B7101A82E0080000000003</UID>",
            "<Start>2026-10-06T08:00:00Z</Start><End>2026-10-06T10:00:00Z</End>",
            "<IsAllDayEvent>false</IsAllDayEvent><LegacyFreeBusyStatus>Busy</LegacyFreeBusyStatus>",
            "<IsCancelled>false</IsCancelled><CalendarItemType>Single</CalendarItemType>",
            "<MyResponseType>Organizer</MyResponseType><IsOrganizer>true</IsOrganizer>"
        ),
    );
    let private = b.mailbox(PRIVATE_ADDRESS, "Private", Access::Denied);
    b.folder(private, None, "Inbox", "IPF.Note", Some("inbox"));

    for rule in [RULE_NEWSLETTERS, RULE_VOICEMAIL] {
        let mut element = xml::parse(rule).unwrap_or_default();
        let id = b.state.new_id("AQAAAJ5R");
        element.children.insert(0, child("RuleId", &id));
        b.state.rules.push(element);
    }
    // Fixture creation is not a change anybody waits for.
    b.state.subscriptions.clear();
    b.state
}

/// A recorded `ErrorServerBusy` fault (HTTP 500).
pub const RECORDED_SERVER_BUSY: &str = concat!(
    r#"<?xml version="1.0" encoding="utf-8"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">"#,
    r#"<s:Body><s:Fault><faultcode xmlns:a="http://schemas.microsoft.com/exchange/services/2006/types">a:ErrorServerBusy</faultcode>"#,
    r#"<faultstring xml:lang="en-US">The server cannot service this request right now. Try again later.</faultstring>"#,
    r#"<detail><e:ResponseCode xmlns:e="http://schemas.microsoft.com/exchange/services/2006/errors">ErrorServerBusy</e:ResponseCode>"#,
    r#"<e:Message xmlns:e="http://schemas.microsoft.com/exchange/services/2006/errors">The server cannot service this request right now. Try again later.</e:Message>"#,
    r#"<t:MessageXml xmlns:t="http://schemas.microsoft.com/exchange/services/2006/types">"#,
    r#"<t:Value Name="BackOffMilliseconds">297749</t:Value></t:MessageXml></detail></s:Fault></s:Body></s:Envelope>"#
);

/// A recorded `ErrorInvalidSyncStateData` answer to `SyncFolderItems`.
pub const RECORDED_INVALID_SYNC_STATE: &str = concat!(
    r#"<?xml version="1.0" encoding="utf-8"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">"#,
    r#"<s:Header><h:ServerVersionInfo MajorVersion="15" MinorVersion="2" MajorBuildNumber="1544" MinorBuildNumber="4" "#,
    r#"Version="V2017_07_11" xmlns:h="http://schemas.microsoft.com/exchange/services/2006/types" "#,
    r#"xmlns="http://schemas.microsoft.com/exchange/services/2006/types"/></s:Header>"#,
    r#"<s:Body><m:SyncFolderItemsResponse xmlns:m="http://schemas.microsoft.com/exchange/services/2006/messages" "#,
    r#"xmlns:t="http://schemas.microsoft.com/exchange/services/2006/types"><m:ResponseMessages>"#,
    r#"<m:SyncFolderItemsResponseMessage ResponseClass="Error"><m:MessageText>Synchronization state data is corrupt or otherwise invalid.</m:MessageText>"#,
    r#"<m:ResponseCode>ErrorInvalidSyncStateData</m:ResponseCode><m:DescriptiveLinkKey>0</m:DescriptiveLinkKey>"#,
    r#"<m:SyncState/><m:IncludesLastItemInRange>true</m:IncludesLastItemInRange></m:SyncFolderItemsResponseMessage>"#,
    r#"</m:ResponseMessages></m:SyncFolderItemsResponse></s:Body></s:Envelope>"#
);

/// A recorded `GetUserOofSettings` answer (scheduled, HTML replies).
pub const RECORDED_OOF: &str = concat!(
    r#"<?xml version="1.0" encoding="utf-8"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">"#,
    r#"<s:Header><h:ServerVersionInfo MajorVersion="15" MinorVersion="2" MajorBuildNumber="1544" MinorBuildNumber="4" "#,
    r#"Version="V2017_07_11" xmlns:h="http://schemas.microsoft.com/exchange/services/2006/types" "#,
    r#"xmlns="http://schemas.microsoft.com/exchange/services/2006/types"/></s:Header>"#,
    r#"<s:Body xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:xsd="http://www.w3.org/2001/XMLSchema">"#,
    r#"<GetUserOofSettingsResponse xmlns="http://schemas.microsoft.com/exchange/services/2006/messages">"#,
    r#"<ResponseMessage ResponseClass="Success"><ResponseCode>NoError</ResponseCode></ResponseMessage>"#,
    r#"<OofSettings xmlns="http://schemas.microsoft.com/exchange/services/2006/types"><OofState>Scheduled</OofState>"#,
    r#"<ExternalAudience>Known</ExternalAudience><Duration><StartTime>2026-10-12T06:00:00Z</StartTime>"#,
    r#"<EndTime>2026-10-16T16:00:00Z</EndTime></Duration><InternalReply><Message>&lt;html&gt;&lt;head&gt;&lt;style&gt;p{margin:0}&lt;/style&gt;&lt;/head&gt;"#,
    r#"&lt;body&gt;&lt;div&gt;I am at the conference until Friday.&lt;/div&gt;&lt;div&gt;Bob covers for me.&lt;/div&gt;&lt;/body&gt;&lt;/html&gt;</Message></InternalReply>"#,
    r#"<ExternalReply><Message>&lt;html&gt;&lt;body&gt;&lt;p&gt;Out of office until 16 October.&lt;/p&gt;&lt;/body&gt;&lt;/html&gt;</Message></ExternalReply>"#,
    r#"</OofSettings><AllowExternalOof>All</AllowExternalOof></GetUserOofSettingsResponse></s:Body></s:Envelope>"#
);

/// A recorded `GetStreamingEvents` stream (two envelopes: new mail, then
/// the server closing the connection).
pub const RECORDED_STREAM: &str = concat!(
    r#"<Envelope xmlns="http://schemas.xmlsoap.org/soap/envelope/"><soap11:Header xmlns:soap11="http://schemas.xmlsoap.org/soap/envelope/">"#,
    r#"<ServerVersionInfo xmlns="http://schemas.microsoft.com/exchange/services/2006/types" MajorVersion="15" MinorVersion="2" MajorBuildNumber="1544" MinorBuildNumber="4" Version="V2017_07_11"/>"#,
    r#"</soap11:Header><soap11:Body xmlns:soap11="http://schemas.xmlsoap.org/soap/envelope/">"#,
    r#"<m:GetStreamingEventsResponse xmlns:m="http://schemas.microsoft.com/exchange/services/2006/messages" xmlns:t="http://schemas.microsoft.com/exchange/services/2006/types">"#,
    r#"<m:ResponseMessages><m:GetStreamingEventsResponseMessage ResponseClass="Success"><m:ResponseCode>NoError</m:ResponseCode>"#,
    r#"<m:Notifications><m:Notification><t:SubscriptionId>EQBtYWlsLmNvcnAuZXhhbXBsZRAAAAA=</t:SubscriptionId>"#,
    r#"<t:NewMailEvent><t:Watermark>AQAAAA==</t:Watermark><t:TimeStamp>2026-10-01T10:00:00Z</t:TimeStamp>"#,
    r#"<t:ItemId Id="AAMkNewMail" ChangeKey="CQAAAA=="/><t:ParentFolderId Id="AQMkInbox" ChangeKey="AQAAAA=="/></t:NewMailEvent>"#,
    r#"<t:CreatedEvent><t:Watermark>AQAAAB==</t:Watermark><t:TimeStamp>2026-10-01T10:00:00Z</t:TimeStamp>"#,
    r#"<t:ItemId Id="AAMkNewMail" ChangeKey="CQAAAA=="/><t:ParentFolderId Id="AQMkInbox" ChangeKey="AQAAAA=="/></t:CreatedEvent>"#,
    r#"</m:Notification></m:Notifications><m:ConnectionStatus>OK</m:ConnectionStatus></m:GetStreamingEventsResponseMessage>"#,
    r#"</m:ResponseMessages></m:GetStreamingEventsResponse></soap11:Body></Envelope>"#,
    r#"<Envelope xmlns="http://schemas.xmlsoap.org/soap/envelope/"><soap11:Body xmlns:soap11="http://schemas.xmlsoap.org/soap/envelope/">"#,
    r#"<m:GetStreamingEventsResponse xmlns:m="http://schemas.microsoft.com/exchange/services/2006/messages"><m:ResponseMessages>"#,
    r#"<m:GetStreamingEventsResponseMessage ResponseClass="Success"><m:ResponseCode>NoError</m:ResponseCode>"#,
    r#"<m:ConnectionStatus>Closed</m:ConnectionStatus></m:GetStreamingEventsResponseMessage></m:ResponseMessages>"#,
    r#"</m:GetStreamingEventsResponse></soap11:Body></Envelope>"#
);

/// A recorded `GetUserAvailability` answer for a person and a room.
pub const RECORDED_AVAILABILITY: &str = concat!(
    r#"<?xml version="1.0" encoding="utf-8"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">"#,
    r#"<s:Body xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:xsd="http://www.w3.org/2001/XMLSchema">"#,
    r#"<GetUserAvailabilityResponse xmlns="http://schemas.microsoft.com/exchange/services/2006/messages"><FreeBusyResponseArray>"#,
    r#"<FreeBusyResponse><ResponseMessage ResponseClass="Success"><ResponseCode>NoError</ResponseCode></ResponseMessage>"#,
    r#"<FreeBusyView><FreeBusyViewType xmlns="http://schemas.microsoft.com/exchange/services/2006/types">FreeBusy</FreeBusyViewType>"#,
    r#"<CalendarEventArray xmlns="http://schemas.microsoft.com/exchange/services/2006/types">"#,
    r#"<CalendarEvent><StartTime>2026-10-05T07:00:00</StartTime><EndTime>2026-10-05T07:30:00</EndTime><BusyType>Busy</BusyType></CalendarEvent>"#,
    r#"<CalendarEvent><StartTime>2026-10-05T12:00:00</StartTime><EndTime>2026-10-05T13:00:00</EndTime><BusyType>Tentative</BusyType></CalendarEvent>"#,
    r#"</CalendarEventArray></FreeBusyView></FreeBusyResponse>"#,
    r#"<FreeBusyResponse><ResponseMessage ResponseClass="Error"><MessageText>Unable to resolve e-mail address &lt;&gt;SMTP:nobody@corp.example to an Active Directory object.</MessageText>"#,
    r#"<ResponseCode>ErrorMailRecipientNotFound</ResponseCode><DescriptiveLinkKey>0</DescriptiveLinkKey>"#,
    r#"<MessageXml><ExceptionType xmlns="http://schemas.microsoft.com/exchange/services/2006/errors">Microsoft.Exchange.InfoWorker.Common.Availability.MailRecipientNotFoundException</ExceptionType></MessageXml>"#,
    r#"</ResponseMessage><FreeBusyView><FreeBusyViewType xmlns="http://schemas.microsoft.com/exchange/services/2006/types">None</FreeBusyViewType></FreeBusyView></FreeBusyResponse>"#,
    r#"</FreeBusyResponseArray></GetUserAvailabilityResponse></s:Body></s:Envelope>"#
);
