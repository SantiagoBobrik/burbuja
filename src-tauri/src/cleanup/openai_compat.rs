//! The OpenAI-compatible chat completions protocol, as spoken by the local servers Ollama and
//! LM Studio. Only ever pointed at a loopback address (see `Cleanup::from_settings`).

use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use super::{ContentPart, LlmError, LlmProvider, LlmRequest, LlmResponse, Role, base64};

#[derive(Debug, Clone)]
pub struct OpenAiCompat {
    pub client: reqwest::Client,
    pub name: &'static str,
    pub base_url: String,
    /// Empty = whatever model the server has loaded (LM Studio accepts `""`).
    pub model: String,
}

/// Set once a server rejects `reasoning_effort` (HTTP 400); later requests leave it out.
static REASONING_PARAM_REJECTED: AtomicBool = AtomicBool::new(false);

/// `no_reasoning` asks thinking models (Gemma 4, Qwen 3…) to answer directly: in LM Studio their
/// hidden reasoning cost 3–12 s per dictation, against ~0.4 s without it.
pub fn request_body(model: &str, req: &LlmRequest, no_reasoning: bool) -> Value {
    let mut messages = vec![json!({ "role": "system", "content": req.system })];
    for m in &req.messages {
        let role = match m.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        };
        // Plain string content when it's text only: the widest server compatibility.
        let content =
            match m.content.as_slice() {
                [ContentPart::Text(t)] => json!(t),
                parts => json!(parts
                .iter()
                .map(|p| match p {
                    ContentPart::Text(t) => json!({ "type": "text", "text": t }),
                    ContentPart::Image { mime, bytes } => json!({
                        "type": "image_url",
                        "image_url": { "url": format!("data:{mime};base64,{}", base64(bytes)) },
                    }),
                })
                .collect::<Vec<_>>()),
            };
        messages.push(json!({ "role": role, "content": content }));
    }
    let mut body = json!({ "model": model, "messages": messages, "max_tokens": req.max_tokens, "temperature": 0 });
    if no_reasoning {
        body["reasoning_effort"] = json!("none");
    }
    body
}

pub fn parse_response(body: &Value) -> Result<String, LlmError> {
    let choice = &body["choices"][0];
    if choice["finish_reason"] == "content_filter" {
        return Err(LlmError::Refusal);
    }
    choice["message"]["content"]
        .as_str()
        .map(|t| t.trim().to_string())
        .ok_or(LlmError::BadResponse)
}

/// Awaits `request` and its JSON body, honoring `cancel` and mapping errors.
pub(crate) async fn send_json(
    request: impl std::future::Future<Output = reqwest::Result<reqwest::Response>>,
    cancel: CancellationToken,
) -> Result<Value, LlmError> {
    let response = tokio::select! {
        _ = cancel.cancelled() => return Err(LlmError::Cancelled),
        r = request => r.map_err(map_reqwest)?,
    };
    if !response.status().is_success() {
        return Err(LlmError::Status(response.status().as_u16()));
    }
    tokio::select! {
        _ = cancel.cancelled() => Err(LlmError::Cancelled),
        b = super::read_json_limited(response, super::MAX_RESPONSE_BYTES) => b,
    }
}

fn map_reqwest(e: reqwest::Error) -> LlmError {
    if e.is_timeout() {
        LlmError::Timeout
    } else {
        LlmError::Network(e.without_url().to_string())
    }
}

impl LlmProvider for OpenAiCompat {
    async fn complete(
        &self,
        req: LlmRequest,
        cancel: CancellationToken,
    ) -> Result<LlmResponse, LlmError> {
        let send = |no_reasoning: bool| {
            self.client
                .post(format!("{}/chat/completions", self.base_url))
                .json(&request_body(&self.model, &req, no_reasoning))
                .timeout(req.timeout)
                .send()
        };
        let no_reasoning = !REASONING_PARAM_REJECTED.load(Ordering::Relaxed);
        let body = match send_json(send(no_reasoning), cancel.clone()).await {
            Err(LlmError::Status(400)) if no_reasoning => {
                log::info!(
                    "{} rejected reasoning_effort; retrying without it",
                    self.name
                );
                REASONING_PARAM_REJECTED.store(true, Ordering::Relaxed);
                send_json(send(false), cancel).await?
            }
            other => other?,
        };
        parse_response(&body).map(|text| LlmResponse { text })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cleanup::Message;
    use std::time::Duration;

    fn req(content: Vec<ContentPart>) -> LlmRequest {
        LlmRequest {
            system: "sys".into(),
            messages: vec![Message {
                role: Role::User,
                content,
            }],
            max_tokens: 100,
            timeout: Duration::from_secs(5),
        }
    }

    #[test]
    fn text_only_uses_string_content_with_system_first() {
        let body = request_body("m", &req(vec![ContentPart::Text("hi".into())]), true);
        assert_eq!(body["reasoning_effort"], "none");
        let plain = request_body("m", &req(vec![ContentPart::Text("hi".into())]), false);
        assert!(plain.get("reasoning_effort").is_none());
        assert_eq!(
            body["messages"][0],
            json!({"role": "system", "content": "sys"})
        );
        assert_eq!(
            body["messages"][1],
            json!({"role": "user", "content": "hi"})
        );
        assert_eq!(body["temperature"], 0);
    }

    #[test]
    fn image_parts_serialize_as_data_urls() {
        let body = request_body(
            "m",
            &req(vec![
                ContentPart::Text("what is this".into()),
                ContentPart::Image {
                    mime: "image/png".into(),
                    bytes: b"foo".to_vec(),
                },
            ]),
            false,
        );
        assert_eq!(
            body["messages"][1]["content"][1],
            json!({"type": "image_url", "image_url": {"url": "data:image/png;base64,Zm9v"}})
        );
    }

    #[test]
    fn parses_content_and_content_filter() {
        let ok = json!({"choices": [{"finish_reason": "stop", "message": {"content": " Hi. "}}]});
        assert_eq!(parse_response(&ok).unwrap(), "Hi.");
        let filtered =
            json!({"choices": [{"finish_reason": "content_filter", "message": {"content": ""}}]});
        assert!(matches!(parse_response(&filtered), Err(LlmError::Refusal)));
    }
}
