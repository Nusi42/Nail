//! [`nail_mcp::CoreData`]: the `McpData` the core gets by implementing
//! `CoreAccess` — requests map to the Client API, raw sources come from the
//! blob store, attachments are cut out with nail-mime.

mod common;

use std::sync::Mutex;

use nail_api::{
    Account, AccountKind, ApiError, ErrorCode, Event, Folder, FolderRole, MessageId, Request,
    Response, SearchQuery, UiContext,
};
use nail_mcp::{CoreAccess, CoreData, McpData, SearchOutcome};
use nail_store::{BlobStore, Store, StoredMessage};

struct FakeCore {
    store: Store,
    blobs: BlobStore,
    requests: Mutex<Vec<String>>,
    events: Mutex<Vec<Event>>,
}

fn name(request: &Request) -> String {
    serde_json::to_value(request).unwrap()["type"]
        .as_str()
        .unwrap()
        .to_owned()
}

impl CoreAccess for FakeCore {
    async fn handle(&self, request: Request) -> Response {
        self.requests.lock().unwrap().push(name(&request));
        match request {
            Request::ListAccounts => Response::Accounts {
                accounts: vec![Account {
                    id: "work".into(),
                    name: "Work".into(),
                    email: "me@work.example".into(),
                    kind: AccountKind::Imap,
                }],
            },
            Request::ListVirtualAccounts => Response::VirtualAccounts {
                virtual_accounts: vec![],
            },
            Request::ListRules => Response::Error {
                code: ErrorCode::Unsupported,
                message: "rules arrive with WP22".into(),
                localized: None,
            },
            Request::GetUiContext => Response::UiContext {
                context: UiContext {
                    message_id: Some("m1".into()),
                    ..UiContext::default()
                },
            },
            // One local hit and one found only by the server search.
            Request::Search { query } => Response::SearchResults {
                offset: query.offset,
                total: 7,
                hits: vec![
                    nail_api::SearchHit {
                        message: common::summary("m1", "work", "inbox", "t1"),
                        score: 1.0,
                        from_server: false,
                    },
                    nail_api::SearchHit {
                        message: common::summary("m9", "work", "lists", "t9"),
                        score: 0.0,
                        from_server: true,
                    },
                ],
                server: Some(nail_api::search::ServerSearch {
                    accounts: vec!["work".into()],
                    hits: 1,
                    failed: vec![],
                }),
            },
            _ => Response::Done,
        }
    }

    fn store(&self) -> &Store {
        &self.store
    }

    fn blobs(&self) -> &BlobStore {
        &self.blobs
    }

    fn emit(&self, event: Event) {
        self.events.lock().unwrap().push(event);
    }

    async fn is_encrypted(&self, id: &MessageId) -> Result<bool, ApiError> {
        // The core answers from its reader; the fake knows m3.
        self.requests
            .lock()
            .unwrap()
            .push(format!("is_encrypted {id}"));
        Ok(id.as_str() == "m3")
    }
}

fn core(dir: &std::path::Path) -> FakeCore {
    let store = Store::open_in_memory().unwrap();
    let blobs = BlobStore::open(dir).unwrap();
    store
        .upsert_account(&Account {
            id: "work".into(),
            name: "Work".into(),
            email: "me@work.example".into(),
            kind: AccountKind::Imap,
        })
        .unwrap();
    store
        .upsert_folder(&Folder {
            id: "work-inbox".into(),
            account_id: "work".into(),
            parent_id: None,
            name: "Inbox".into(),
            path: "INBOX".into(),
            role: FolderRole::Inbox,
            unread: 0,
            total: 0,
        })
        .unwrap();
    let mut m1 = StoredMessage::new(common::summary("m1", "work", "work-inbox", "t1"));
    m1.blob_hash = Some(blobs.put(common::RAW_M1).unwrap());
    store.upsert_message(&m1).unwrap();
    let encrypted = b"Content-Type: multipart/encrypted; protocol=\"application/pgp-encrypted\"\r\n\r\n--x--\r\n";
    let mut m3 = StoredMessage::new(common::summary("m3", "work", "work-inbox", "t3"));
    m3.blob_hash = Some(blobs.put(encrypted).unwrap());
    store.upsert_message(&m3).unwrap();
    FakeCore {
        store,
        blobs,
        requests: Mutex::new(Vec::new()),
        events: Mutex::new(Vec::new()),
    }
}

#[tokio::test]
async fn core_data_reads_through_requests_store_and_blobs() {
    let dir = tempfile::tempdir().unwrap();
    let data = CoreData(core(dir.path()));
    let m1 = MessageId::new("m1");

    assert_eq!(data.accounts().await.unwrap()[0].id.as_str(), "work");
    assert!(data.virtual_accounts().await.unwrap().is_empty());
    assert_eq!(
        data.message_summary(&m1).await.unwrap().subject,
        "Subject of m1"
    );
    assert_eq!(data.raw_message(&m1).await.unwrap(), common::RAW_M1);

    let attachment = data.attachment(&m1, 0).await.unwrap();
    assert_eq!(attachment.filename.as_deref(), Some("plan.csv"));
    assert_eq!(attachment.mime, "text/csv");
    assert_eq!(attachment.data, b"a,b");
    assert_eq!(
        data.attachment(&m1, 7).await.unwrap_err().code,
        ErrorCode::NotFound
    );

    // The encrypted gate is the core's answer (its reader), not a guess
    // from the raw bytes.
    assert!(!data.is_encrypted(&m1).await.unwrap());
    assert!(data.is_encrypted(&"m3".into()).await.unwrap());
    assert!(
        data.0
            .requests
            .lock()
            .unwrap()
            .contains(&"is_encrypted m3".to_owned())
    );
    assert_eq!(
        data.message_summary(&"nope".into()).await.unwrap_err().code,
        ErrorCode::NotFound
    );

    // Errors and unexpected answers of the core become ApiErrors.
    let err: ApiError = data.rules_text().await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Unsupported);
    let err = data.folders(&"work".into()).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Internal, "Done is not Folders");

    assert_eq!(
        data.ui_context().await.unwrap().message_id,
        Some("m1".into())
    );
    let query = SearchQuery {
        text: "x".into(),
        account_id: None,
        folder_id: None,
        offset: 3,
        limit: 5,
        sort: Default::default(),
    };
    // MCP pages over local hits only: server hits and their count go.
    let SearchOutcome::Hits {
        offset,
        total,
        hits,
    } = data.search(query).await.unwrap()
    else {
        panic!("no hits")
    };
    assert_eq!((offset, total), (3, 6));
    assert_eq!(hits.len(), 1);
    assert!(!hits[0].from_server);

    data.emit(Event::McpClientsChanged);
    assert_eq!(data.0.events.lock().unwrap().len(), 1);
    let requests = data.0.requests.lock().unwrap().clone();
    assert!(requests.contains(&"ListRules".to_owned()), "{requests:?}");
}
