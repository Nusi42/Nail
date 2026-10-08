//! Batched writes with a one-by-one fallback, as the Gmail, Graph and EWS
//! engines replay them: the writes a batch call answered keep that answer,
//! the rest go one at a time, and the results come back in order. One
//! definition for the three engines (wave Q2, ADR 0171).

/// Records `error` as the answer of every write in `indices` (the batch
/// call that carried them failed as a whole).
pub fn fail_all<T, E: Clone>(
    answers: &mut [Option<Result<T, E>>],
    indices: impl IntoIterator<Item = usize>,
    error: &E,
) {
    for index in indices {
        if let Some(slot) = answers.get_mut(index) {
            *slot = Some(Err(error.clone()));
        }
    }
}

/// The result of every write, in order: the answer a batch call recorded
/// for it, else `single(write)`, awaited one after the other.
pub async fn finish<'a, W, T, Fut>(
    writes: &'a [W],
    mut answers: Vec<Option<T>>,
    mut single: impl FnMut(&'a W) -> Fut,
) -> Vec<T>
where
    Fut: Future<Output = T>,
{
    let mut results = Vec::with_capacity(writes.len());
    for (index, write) in writes.iter().enumerate() {
        let result = match answers.get_mut(index).and_then(Option::take) {
            Some(result) => result,
            None => single(write).await,
        };
        results.push(result);
    }
    results
}

#[cfg(test)]
mod tests {
    use std::pin::pin;
    use std::task::{Context, Poll, Waker};

    use super::*;

    /// Polls a future that never waits to its end.
    fn ready<T>(future: impl Future<Output = T>) -> T {
        let mut future = pin!(future);
        let mut cx = Context::from_waker(Waker::noop());
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("the future waited"),
        }
    }

    #[test]
    fn recorded_answers_win_and_the_rest_go_one_by_one() {
        let writes = ["a", "b", "c", "d"];
        let mut answers: Vec<Option<Result<String, String>>> = vec![None; writes.len()];
        answers[0] = Some(Ok("batched a".into()));
        fail_all(&mut answers, [2, 9], &"batch failed".to_owned());
        let mut singles = Vec::new();
        let results = ready(finish(&writes, answers, |w| {
            singles.push(*w);
            async move { Ok(format!("single {w}")) }
        }));
        assert_eq!(
            results,
            [
                Ok("batched a".to_owned()),
                Ok("single b".to_owned()),
                Err("batch failed".to_owned()),
                Ok("single d".to_owned()),
            ]
        );
        assert_eq!(singles, ["b", "d"]);
    }
}
