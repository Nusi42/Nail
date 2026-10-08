//! JSON batching (`POST /$batch`): up to [`MAX_BATCH`] requests per call,
//! answers matched by id, throttled parts retried in a later round with
//! their `Retry-After`.

use std::collections::BTreeMap;

use nail_net::status as http_status;
use reqwest::Method;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use nail_net::http::{Body, parse_retry_after};

use crate::client::{Call, GraphClient, parse_json};
use crate::error::GraphError;
use crate::wire::ErrorBody;

/// Graph's limit of requests in one `$batch`.
pub const MAX_BATCH: usize = 20;

/// One request of a batch.
#[derive(Debug, Clone, PartialEq)]
pub struct BatchRequest {
    /// `GET`, `POST`, `PATCH`, `DELETE`.
    pub method: Method,
    /// The URL relative to the version root (`/me/mailFolders/inbox`).
    pub url: String,
    /// Extra headers (`Prefer`, …).
    pub headers: Vec<(String, String)>,
    /// A JSON body.
    pub body: Option<Value>,
}

impl BatchRequest {
    /// A `GET` of `url`.
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            method: Method::GET,
            url: url.into(),
            headers: Vec::new(),
            body: None,
        }
    }

    /// A request with a JSON body.
    pub fn with_body(method: Method, url: impl Into<String>, body: Value) -> Self {
        Self {
            method,
            url: url.into(),
            headers: Vec::new(),
            body: Some(body),
        }
    }

    /// Adds a header.
    #[must_use]
    pub fn header(mut self, name: &str, value: impl Into<String>) -> Self {
        nail_net::http::push_header(&mut self.headers, name, value);
        self
    }

    fn to_json(&self, id: usize) -> Value {
        let mut headers = serde_json::Map::new();
        for (name, value) in crate::client::with_immutable_ids(&self.headers) {
            headers.insert(name, Value::String(value));
        }
        if self.body.is_some() {
            headers.insert("Content-Type".into(), "application/json".into());
        }
        let mut request = json!({
            "id": id.to_string(),
            "method": self.method.as_str(),
            "url": self.url,
        });
        if !headers.is_empty() {
            request["headers"] = Value::Object(headers);
        }
        if let Some(body) = &self.body {
            request["body"] = body.clone();
        }
        request
    }
}

/// The answer to one request of a batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchResponse {
    /// HTTP status.
    pub status: u16,
    /// Headers (names lowercase).
    pub headers: BTreeMap<String, String>,
    /// The JSON body (`Null` when there is none).
    pub body: Value,
}

impl BatchResponse {
    /// Whether the request succeeded.
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// The `Retry-After` of a throttled part.
    pub fn retry_after(&self) -> Option<u32> {
        self.headers
            .get("retry-after")
            .and_then(|v| parse_retry_after(v, chrono::Utc::now()))
    }

    /// The part's error (`None` when it succeeded).
    pub fn error(&self, what: &str) -> Option<GraphError> {
        if self.ok() {
            return None;
        }
        let body: Option<ErrorBody> = serde_json::from_value(self.body.clone()).ok();
        Some(GraphError::from_status(
            self.status,
            self.retry_after(),
            body.as_ref(),
            what,
        ))
    }

    /// The body as `T`, or the part's error.
    pub fn json<T: DeserializeOwned>(&self, what: &str) -> Result<T, GraphError> {
        if let Some(error) = self.error(what) {
            return Err(error);
        }
        serde_json::from_value(self.body.clone())
            .map_err(|e| GraphError::Protocol(format!("{what}: {e}")))
    }
}

#[derive(Debug, Deserialize)]
struct WireAnswer {
    #[serde(default)]
    responses: Vec<WireResponse>,
}

#[derive(Debug, Deserialize)]
struct WireResponse {
    id: String,
    status: u16,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default)]
    body: Value,
}

/// Whether a part may be repeated after it failed with `status`.
fn retry_part(request: &BatchRequest, status: u16) -> bool {
    match status {
        http_status::TOO_MANY_REQUESTS => true,
        500 | 502 | 503 | http_status::GATEWAY_TIMEOUT => request.method != Method::POST,
        _ => false,
    }
}

impl GraphClient {
    /// Sends `requests` as `$batch` calls of at most [`MAX_BATCH`] and
    /// answers one [`BatchResponse`] per request, in order. Throttled parts
    /// (and server errors of repeatable parts) are sent again in a later
    /// round, waiting for the longest `Retry-After`; a failed batch call
    /// fails the whole operation.
    pub async fn batch(&self, requests: &[BatchRequest]) -> Result<Vec<BatchResponse>, GraphError> {
        let mut answers: Vec<Option<BatchResponse>> = vec![None; requests.len()];
        let mut pending: Vec<usize> = (0..requests.len()).collect();
        let retry = self.retry_policy();
        let mut attempt = 0u32;
        while !pending.is_empty() {
            let mut again = Vec::new();
            let mut wait_secs: Option<u32> = None;
            for chunk in pending.chunks(MAX_BATCH) {
                let body = json!({
                    "requests": chunk.iter().map(|&i| requests[i].to_json(i)).collect::<Vec<_>>()
                });
                let call = Call {
                    method: Method::POST,
                    url: self.batch_url()?,
                    body: Body::Json(
                        serde_json::to_vec(&body)
                            .map_err(|e| GraphError::Protocol(e.to_string()))?,
                    ),
                    headers: Vec::new(),
                    // Only throttling of the whole batch is repeated; a
                    // batch that failed with a 5xx may have run parts.
                    idempotent: chunk.iter().all(|&i| requests[i].method == Method::GET),
                    what: "POST /$batch".to_owned(),
                };
                let reply = self.execute(&call).await?;
                let answer: WireAnswer = parse_json(&reply.body, "POST /$batch")?;
                for part in answer.responses {
                    let Some(index) = part.id.parse::<usize>().ok().filter(|i| chunk.contains(i))
                    else {
                        return Err(GraphError::Protocol(format!(
                            "$batch answered an unknown id {}",
                            part.id
                        )));
                    };
                    let response = BatchResponse {
                        status: part.status,
                        headers: part
                            .headers
                            .into_iter()
                            .map(|(k, v)| (k.to_ascii_lowercase(), v))
                            .collect(),
                        body: part.body,
                    };
                    if retry_part(&requests[index], response.status) && attempt < retry.max_retries
                    {
                        wait_secs = wait_secs.max(response.retry_after());
                        again.push(index);
                    }
                    answers[index] = Some(response);
                }
                if let Some(missing) = chunk.iter().find(|&&i| answers[i].is_none()) {
                    return Err(GraphError::Protocol(format!(
                        "$batch did not answer request {missing}"
                    )));
                }
            }
            if again.is_empty() {
                break;
            }
            // A wait longer than the inline limit leaves the throttled
            // answers as they are: the caller sees their errors.
            let Some(wait) = retry.wait(attempt, wait_secs) else {
                break;
            };
            tracing::debug!(parts = again.len(), ?wait, "retrying throttled batch parts");
            tokio::time::sleep(wait).await;
            attempt += 1;
            pending = again;
        }
        answers
            .into_iter()
            .enumerate()
            .map(|(i, a)| a.ok_or_else(|| GraphError::Protocol(format!("no answer for {i}"))))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_serialize_like_graph_expects() {
        let get =
            BatchRequest::get("/me/mailFolders/inbox").header("Prefer", "outlook.timezone=\"UTC\"");
        assert_eq!(
            get.to_json(3),
            json!({"id": "3", "method": "GET", "url": "/me/mailFolders/inbox",
                   "headers": {"Prefer": "IdType=\"ImmutableId\", outlook.timezone=\"UTC\""}})
        );
        let patch =
            BatchRequest::with_body(Method::PATCH, "/me/messages/m1", json!({"isRead": true}));
        assert_eq!(
            patch.to_json(0),
            json!({"id": "0", "method": "PATCH", "url": "/me/messages/m1",
                   "headers": {"Content-Type": "application/json",
                               "Prefer": "IdType=\"ImmutableId\""},
                   "body": {"isRead": true}})
        );
    }

    #[test]
    fn every_request_asks_for_immutable_ids() {
        let merged = crate::client::with_immutable_ids(&[
            ("Prefer".into(), "odata.maxpagesize=50".into()),
            ("If-Match".into(), "W/\"ck\"".into()),
            (
                "prefer".into(),
                "outlook.timezone=\"UTC\", IdType=\"RestId\"".into(),
            ),
        ]);
        assert_eq!(
            merged,
            vec![
                ("If-Match".to_owned(), "W/\"ck\"".to_owned()),
                (
                    "Prefer".to_owned(),
                    "IdType=\"ImmutableId\", odata.maxpagesize=50, outlook.timezone=\"UTC\""
                        .to_owned()
                ),
            ]
        );
        assert_eq!(
            crate::client::with_immutable_ids(&[]),
            vec![("Prefer".to_owned(), "IdType=\"ImmutableId\"".to_owned())]
        );
    }

    #[test]
    fn responses_and_retries() {
        let throttled = BatchResponse {
            status: 429,
            headers: BTreeMap::from([("retry-after".to_owned(), "4".to_owned())]),
            body: json!({"error": {"code": "TooManyRequests", "message": "slow"}}),
        };
        assert!(!throttled.ok());
        assert_eq!(throttled.retry_after(), Some(4));
        assert_eq!(throttled.error("x"), Some(GraphError::Throttled(Some(4))));
        let ok = BatchResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: json!({"id": "f1"}),
        };
        assert_eq!(ok.json::<Value>("x").unwrap()["id"], "f1");
        assert!(ok.error("x").is_none());
        assert!(retry_part(&BatchRequest::get("/me"), 503));
        assert!(!retry_part(
            &BatchRequest::with_body(Method::POST, "/me/sendMail", json!({})),
            503
        ));
        assert!(retry_part(
            &BatchRequest::with_body(Method::POST, "/me/sendMail", json!({})),
            429
        ));
        assert!(!retry_part(&BatchRequest::get("/me"), 404));
    }
}
