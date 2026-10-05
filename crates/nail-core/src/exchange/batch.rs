//! Grouped mail writes of Exchange accounts (P3, ADR 0164): the engines'
//! multi-write calls behind one seam, so the replay can send a
//! conversation's moves and flag changes together — Graph as `$batch`
//! parts (20 per call), EWS as one `MoveItem` / `UpdateItem` /
//! `DeleteItem` with many item ids. The `ExchangeEngine` trait (in
//! `nail-api`) has no such call; the core keeps the concrete engine beside
//! the trait object instead. Engines injected by tests have none and
//! replay one write at a time.

use std::sync::Arc;

use nail_api::engine::{BoxFuture, EngineError, RemoteWrite, WriteOutcome};
use nail_proto_ews::EwsEngine;
use nail_proto_graph::GraphEngine;

/// Several mail writes of one mailbox in as few calls as the engine
/// allows.
pub(crate) trait BatchWrites: Send + Sync + std::fmt::Debug {
    /// One result per write, in order.
    fn write_many<'a>(
        &'a self,
        mailbox: Option<&'a str>,
        writes: &'a [RemoteWrite],
    ) -> BoxFuture<'a, Vec<Result<WriteOutcome, EngineError>>>;
}

impl BatchWrites for GraphEngine {
    fn write_many<'a>(
        &'a self,
        mailbox: Option<&'a str>,
        writes: &'a [RemoteWrite],
    ) -> BoxFuture<'a, Vec<Result<WriteOutcome, EngineError>>> {
        Box::pin(async move {
            GraphEngine::write_many(self, mailbox, writes)
                .await
                .into_iter()
                .map(|r| r.map_err(EngineError::from))
                .collect()
        })
    }
}

impl BatchWrites for EwsEngine {
    fn write_many<'a>(
        &'a self,
        mailbox: Option<&'a str>,
        writes: &'a [RemoteWrite],
    ) -> BoxFuture<'a, Vec<Result<WriteOutcome, EngineError>>> {
        Box::pin(async move {
            EwsEngine::write_many(self, mailbox, writes)
                .await
                .into_iter()
                .map(|r| r.map_err(EngineError::from))
                .collect()
        })
    }
}

/// The batch seam of an engine built by the core.
pub(crate) fn of_graph(engine: &Arc<GraphEngine>) -> Arc<dyn BatchWrites> {
    let engine: Arc<_> = Arc::clone(engine);
    let seam: Arc<dyn BatchWrites> = engine;
    seam
}

/// The batch seam of an engine built by the core.
pub(crate) fn of_ews(engine: &Arc<EwsEngine>) -> Arc<dyn BatchWrites> {
    let engine: Arc<_> = Arc::clone(engine);
    let seam: Arc<dyn BatchWrites> = engine;
    seam
}
