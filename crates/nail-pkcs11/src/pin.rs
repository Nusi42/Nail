//! Asking for PINs.
//!
//! Card operations run on blocking threads (signing and decrypting are
//! synchronous in `nail-pgp` / `nail-smime`), while the core asks the user
//! asynchronously ([`PinSource`]: `Event::PinRequested`, answered with
//! `AnswerPinPrompt`). [`PinPrompter`] is the synchronous side;
//! [`BlockingPinSource`] adapts a [`PinSource`] by waiting for its future
//! on the calling thread. Call card operations from a blocking thread of
//! the runtime (`spawn_blocking`), never from an async task.

use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::thread::Thread;

use nail_api::ApiError;
use nail_api::smartcard::{PinRequest, PinSource};
use zeroize::Zeroizing;

/// Asks the user for a PIN and waits for the answer. `Ok(None)` = the user
/// cancelled (or the prompt timed out).
pub trait PinPrompter: Send + Sync + std::fmt::Debug {
    /// Asks; the PIN is returned in a zeroing buffer.
    fn request_pin(&self, request: PinRequest) -> Result<Option<Zeroizing<String>>, ApiError>;
}

/// A [`PinPrompter`] over the core's asynchronous [`PinSource`].
#[derive(Debug, Clone)]
pub struct BlockingPinSource {
    source: Arc<dyn PinSource>,
}

impl BlockingPinSource {
    /// Wraps a pin source.
    pub fn new(source: Arc<dyn PinSource>) -> Self {
        Self { source }
    }
}

impl PinPrompter for BlockingPinSource {
    fn request_pin(&self, request: PinRequest) -> Result<Option<Zeroizing<String>>, ApiError> {
        block_on(self.source.request_pin(request)).map(|pin| pin.map(Zeroizing::new))
    }
}

struct ThreadWaker(Thread);

impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

/// Runs a future to completion on the current thread (parking between
/// polls). Wakers from any executor's primitives (tokio channels and
/// timers inside the runtime's blocking threads) work.
pub fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let waker = Waker::from(Arc::new(ThreadWaker(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(out) => return out,
            Poll::Pending => std::thread::park(),
        }
    }
}

/// A prompter that always answers the same (tests, scripted runs). Counts
/// the requests it saw.
#[derive(Debug, Default)]
pub struct FixedPin {
    answers: std::sync::Mutex<std::collections::VecDeque<Option<String>>>,
    seen: std::sync::Mutex<Vec<PinRequest>>,
}

impl FixedPin {
    /// Answers with these PINs in order (`None` = cancel), then cancels.
    pub fn answers(answers: impl IntoIterator<Item = Option<&'static str>>) -> Self {
        Self {
            answers: std::sync::Mutex::new(
                answers.into_iter().map(|a| a.map(str::to_owned)).collect(),
            ),
            seen: std::sync::Mutex::default(),
        }
    }

    /// The requests asked so far.
    pub fn requests(&self) -> Vec<PinRequest> {
        self.seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl PinPrompter for FixedPin {
    fn request_pin(&self, request: PinRequest) -> Result<Option<Zeroizing<String>>, ApiError> {
        self.seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(request);
        Ok(self
            .answers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pop_front()
            .flatten()
            .map(Zeroizing::new))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::engine::BoxFuture;
    use nail_api::smartcard::PinPurpose;

    #[derive(Debug)]
    struct ThreadedSource;

    impl PinSource for ThreadedSource {
        fn request_pin(
            &self,
            request: PinRequest,
        ) -> BoxFuture<'_, Result<Option<String>, ApiError>> {
            // Answers from another thread after a moment, like a UI would.
            let state = Arc::new(std::sync::Mutex::new((
                None::<Option<String>>,
                None::<Waker>,
            )));
            let shared = Arc::clone(&state);
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(20));
                let answer = if request.retry {
                    None
                } else {
                    Some("123456".to_owned())
                };
                let mut guard = shared.lock().unwrap();
                guard.0 = Some(answer);
                if let Some(waker) = guard.1.take() {
                    waker.wake();
                }
            });
            Box::pin(std::future::poll_fn(move |cx| {
                let mut guard = state.lock().unwrap();
                if let Some(answer) = guard.0.take() {
                    Poll::Ready(Ok(answer))
                } else {
                    guard.1 = Some(cx.waker().clone());
                    Poll::Pending
                }
            }))
        }
    }

    fn request(retry: bool) -> PinRequest {
        PinRequest {
            token_label: "Card".into(),
            token_serial: "1".into(),
            purpose: PinPurpose::Sign,
            key_label: None,
            attempts_left: None,
            retry,
        }
    }

    #[test]
    fn blocking_source_waits_for_the_async_answer() {
        let prompter = BlockingPinSource::new(Arc::new(ThreadedSource));
        assert_eq!(
            prompter
                .request_pin(request(false))
                .unwrap()
                .as_deref()
                .map(String::as_str),
            Some("123456")
        );
        assert!(prompter.request_pin(request(true)).unwrap().is_none());
    }

    #[test]
    fn fixed_pins_answer_in_order_and_record_requests() {
        let pins = FixedPin::answers([Some("1111"), None]);
        assert_eq!(
            pins.request_pin(request(false)).unwrap().unwrap().as_str(),
            "1111"
        );
        assert!(pins.request_pin(request(true)).unwrap().is_none());
        assert!(pins.request_pin(request(true)).unwrap().is_none());
        assert_eq!(pins.requests().len(), 3);
    }
}
