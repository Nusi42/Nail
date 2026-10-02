//! Overlapping fetch → prepare → store steps for batches (post-M4c
//! performance work, ADR 0101).
//!
//! [`pipeline`] runs `count` batches through three steps that overlap:
//!
//! 1. **fetch** ([`BatchSource::fetch`], e.g. one `UID FETCH` on the IMAP
//!    connection),
//! 2. **prepare** (a blocking function on the blocking pool, e.g. blob
//!    writes and MIME parsing on worker threads),
//! 3. **store** ([`BatchSink::store`], e.g. one store transaction plus the
//!    folder's checkpoint).
//!
//! While batch *n* is stored, batch *n + 1* is prepared and batch *n + 2*
//! fetched. Batches reach the sink strictly in order. A failing step stops
//! the others: batches fetched before a fetch error are still prepared and
//! stored; after a prepare or store error nothing more is stored. The
//! error returned is the one furthest down the pipeline (the store's, then
//! the prepare step's, then the fetch's), since it is what stopped the
//! steps before it.

use std::future::Future;

use nail_api::ApiError;

use crate::error::{CoreError, Result};

/// Batches waiting between two steps (fetched → prepared → stored); bounds
/// the memory of the overlap.
pub(crate) const PIPELINE_DEPTH: usize = 1;

/// The fetching step of [`pipeline`].
pub(crate) trait BatchSource {
    /// A fetched batch.
    type Batch: Send + 'static;

    /// Fetches batch `index` (called for 0, 1, … in order).
    fn fetch(&mut self, index: usize) -> impl Future<Output = Result<Self::Batch>> + Send;
}

/// The storing step of [`pipeline`].
pub(crate) trait BatchSink<P> {
    /// Stores prepared batch `index` (called in order).
    fn store(&mut self, index: usize, prepared: P) -> impl Future<Output = Result<()>> + Send;
}

/// Runs batches `0..count` through `source`, `prepare` (blocking, on the
/// blocking pool) and `sink`, overlapping the three steps (see the module
/// documentation).
pub(crate) async fn pipeline<S, F, P, K>(
    count: usize,
    source: &mut S,
    prepare: F,
    sink: &mut K,
) -> Result<()>
where
    S: BatchSource + Send,
    F: Fn(S::Batch) -> Result<P> + Clone + Send + 'static,
    P: Send + 'static,
    K: BatchSink<P> + Send,
{
    if count == 0 {
        return Ok(());
    }
    let (fetched_tx, mut fetched_rx) =
        tokio::sync::mpsc::channel::<(usize, S::Batch)>(PIPELINE_DEPTH);
    let (prepared_tx, mut prepared_rx) = tokio::sync::mpsc::channel::<(usize, P)>(PIPELINE_DEPTH);

    let fetching = async {
        let fetched_tx = fetched_tx;
        for index in 0..count {
            let batch = source.fetch(index).await?;
            if fetched_tx.send((index, batch)).await.is_err() {
                // A later step stopped; its error is reported.
                break;
            }
        }
        Ok::<(), CoreError>(())
    };

    let preparing = async {
        let prepared_tx = prepared_tx;
        let result = async {
            while let Some((index, batch)) = fetched_rx.recv().await {
                let prepare = prepare.clone();
                let prepared = tokio::task::spawn_blocking(move || prepare(batch))
                    .await
                    .map_err(|e| ApiError::internal(format!("preparing a batch failed: {e}")))??;
                if prepared_tx.send((index, prepared)).await.is_err() {
                    break;
                }
            }
            Ok::<(), CoreError>(())
        }
        .await;
        // Unblocks the fetching step when this one stopped early.
        fetched_rx.close();
        result
    };

    let storing = async {
        let result = async {
            while let Some((index, prepared)) = prepared_rx.recv().await {
                sink.store(index, prepared).await?;
            }
            Ok::<(), CoreError>(())
        }
        .await;
        prepared_rx.close();
        result
    };

    let (fetched, prepared, stored) = tokio::join!(fetching, preparing, storing);
    stored?;
    prepared?;
    fetched
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use nail_api::ErrorCode;
    use tokio::sync::Notify;

    use super::*;

    /// Fetches `i * 10`, failing at `fail_at`; counts its calls.
    struct Source {
        fail_at: Option<usize>,
        fetched: Arc<Mutex<Vec<usize>>>,
        /// Notified when batch 1 is fetched.
        second: Arc<Notify>,
    }

    impl Source {
        fn new(fail_at: Option<usize>) -> Self {
            Self {
                fail_at,
                fetched: Arc::default(),
                second: Arc::new(Notify::new()),
            }
        }
    }

    impl BatchSource for Source {
        type Batch = usize;

        async fn fetch(&mut self, index: usize) -> Result<usize> {
            if self.fail_at == Some(index) {
                return Err(ApiError::new(ErrorCode::Network, "connection lost").into());
            }
            self.fetched.lock().unwrap().push(index);
            if index == 1 {
                self.second.notify_one();
            }
            Ok(index * 10)
        }
    }

    #[derive(Default)]
    struct Sink {
        stored: Vec<(usize, String)>,
        fail_at: Option<usize>,
        /// Batch 0 waits until batch 1 was fetched (proves the overlap).
        wait_for: Option<Arc<Notify>>,
    }

    impl BatchSink<String> for Sink {
        async fn store(&mut self, index: usize, prepared: String) -> Result<()> {
            if index == 0
                && let Some(second) = self.wait_for.take()
            {
                second.notified().await;
            }
            if self.fail_at == Some(index) {
                return Err(ApiError::new(ErrorCode::Internal, "disk full").into());
            }
            self.stored.push((index, prepared));
            Ok(())
        }
    }

    fn prepare(batch: usize) -> Result<String> {
        Ok(format!("p{batch}"))
    }

    async fn run<K: BatchSink<String> + Send>(
        count: usize,
        source: &mut Source,
        prepare: impl Fn(usize) -> Result<String> + Clone + Send + 'static,
        sink: &mut K,
    ) -> Result<()> {
        tokio::time::timeout(
            Duration::from_secs(30),
            pipeline(count, source, prepare, sink),
        )
        .await
        .expect("the pipeline does not hang")
    }

    #[tokio::test]
    async fn batches_are_stored_in_order_while_the_next_is_fetched() {
        let mut source = Source::new(None);
        let mut sink = Sink {
            wait_for: Some(Arc::clone(&source.second)),
            ..Sink::default()
        };
        // Storing batch 0 waits for the fetch of batch 1: without the
        // overlap this would never finish.
        run(6, &mut source, prepare, &mut sink).await.unwrap();
        let expected: Vec<(usize, String)> = (0..6).map(|i| (i, format!("p{}", i * 10))).collect();
        assert_eq!(sink.stored, expected);
        assert_eq!(*source.fetched.lock().unwrap(), (0..6).collect::<Vec<_>>());
        // Nothing to do.
        let mut empty = Sink::default();
        run(0, &mut Source::new(Some(0)), prepare, &mut empty)
            .await
            .unwrap();
        assert!(empty.stored.is_empty());
    }

    #[tokio::test]
    async fn batches_fetched_before_a_fetch_error_are_stored() {
        let mut source = Source::new(Some(3));
        let mut sink = Sink::default();
        let err = run(6, &mut source, prepare, &mut sink).await.unwrap_err();
        assert_eq!(err.code(), ErrorCode::Network);
        assert_eq!(
            sink.stored.iter().map(|(i, _)| *i).collect::<Vec<_>>(),
            [0, 1, 2]
        );
    }

    #[tokio::test]
    async fn a_store_error_stops_fetching_and_wins() {
        let mut source = Source::new(Some(40));
        let mut sink = Sink {
            fail_at: Some(1),
            ..Sink::default()
        };
        let err = run(50, &mut source, prepare, &mut sink).await.unwrap_err();
        assert_eq!(err.code(), ErrorCode::Internal);
        assert_eq!(sink.stored.len(), 1);
        // Fetching stopped soon after (the steps hold at most a few
        // batches), long before batch 40 could fail.
        let fetched = source.fetched.lock().unwrap().len();
        assert!(fetched <= 1 + 2 * (PIPELINE_DEPTH + 1) + 1, "{fetched}");
    }

    #[tokio::test]
    async fn prepare_errors_and_panics_stop_the_pipeline() {
        let failing = |batch: usize| {
            if batch == 20 {
                Err(ApiError::new(ErrorCode::InvalidRequest, "unparsable").into())
            } else {
                Ok(format!("p{batch}"))
            }
        };
        let mut sink = Sink::default();
        let err = run(30, &mut Source::new(None), failing, &mut sink)
            .await
            .unwrap_err();
        assert_eq!(err.code(), ErrorCode::InvalidRequest);
        assert_eq!(
            sink.stored.iter().map(|(i, _)| *i).collect::<Vec<_>>(),
            [0, 1]
        );

        let panicking = |batch: usize| -> Result<String> {
            assert!(batch != 10, "a bug in the prepare step");
            Ok(String::new())
        };
        let mut sink = Sink::default();
        let err = run(30, &mut Source::new(None), panicking, &mut sink)
            .await
            .unwrap_err();
        assert_eq!(err.code(), ErrorCode::Internal);
        assert_eq!(sink.stored.len(), 1);
    }
}
