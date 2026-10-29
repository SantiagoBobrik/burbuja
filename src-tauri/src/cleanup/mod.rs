//! LLM cleanup of raw transcripts and the provider trait that the future
//! "ask about my screen" feature reuses: requests carry text **and** image parts.
//!
//! Fully local: the only providers are OpenAI-compatible servers on this
//! computer (Ollama, LM Studio). Base URLs must be loopback, and the HTTP client neither
//! follows redirects nor uses a proxy, so the transcript never leaves the device.

pub mod openai_compat;
pub mod prompt;

use std::future::Future;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::core::guards::accept_cleanup;
use crate::events::ErrorCode;
use crate::settings::{CleanupProvider, CleanupStyle, Settings, is_loopback_url};
use openai_compat::OpenAiCompat;

pub const OLLAMA_URL: &str = "http://localhost:11434/v1";
pub const LMSTUDIO_URL: &str = "http://localhost:1234/v1";

#[derive(Debug, Clone, PartialEq)]
pub enum ContentPart {
    Text(String),
    Image { mime: String, bytes: Vec<u8> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentPart>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LlmRequest {
    pub system: String,
    pub messages: Vec<Message>,
    pub max_tokens: u32,
    pub timeout: Duration,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LlmResponse {
    pub text: String,
}

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("cancelled")]
    Cancelled,
    #[error("timed out")]
    Timeout,
    #[error("the model declined")]
    Refusal,
    #[error("network: {0}")]
    Network(String),
    #[error("server returned {0}")]
    Status(u16),
    #[error("unexpected response")]
    BadResponse,
}

impl LlmError {
    pub fn code(&self) -> ErrorCode {
        match self {
            LlmError::Timeout => ErrorCode::ServiceTimeout,
            LlmError::Network(_) => ErrorCode::Offline,
            LlmError::Cancelled
            | LlmError::Refusal
            | LlmError::Status(_)
            | LlmError::BadResponse => ErrorCode::ProviderError,
        }
    }
}

pub trait LlmProvider {
    fn complete(
        &self,
        req: LlmRequest,
        cancel: CancellationToken,
    ) -> impl Future<Output = Result<LlmResponse, LlmError>> + Send;
}

/// The HTTP client for local LLM servers: no proxy (a system proxy would see the transcript),
/// no redirects (a redirect could point off the device), and `localhost` pinned to the
/// loopback addresses so no resolver (hosts file, DNS) can send it elsewhere.
pub fn local_client() -> reqwest::Client {
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
    // Port 0: the port always comes from the URL.
    let loopback = [
        SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        SocketAddr::from((Ipv6Addr::LOCALHOST, 0)),
    ];
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .resolve_to_addrs("localhost", &loopback)
        .build()
        .unwrap_or_else(|e| {
            // Practically unreachable (TLS backend init). Fall back to a client that still
            // never proxies or redirects.
            log::warn!("local LLM client: {e}");
            reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap_or_default()
        })
}

/// Largest response body read from a local LLM server (a cleanup is at most a few KB).
pub(crate) const MAX_RESPONSE_BYTES: usize = 4 << 20;

/// Reads a JSON body of at most `limit` bytes; anything larger is a bad response.
pub(crate) async fn read_json_limited(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<serde_json::Value, LlmError> {
    if response.content_length().is_some_and(|n| n > limit as u64) {
        return Err(LlmError::BadResponse);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| {
        if e.is_timeout() {
            LlmError::Timeout
        } else {
            LlmError::BadResponse
        }
    })? {
        if body.len() + chunk.len() > limit {
            return Err(LlmError::BadResponse);
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|_| LlmError::BadResponse)
}

/// The AI cleanup as configured in settings: which local server and model, the vocabulary, the
/// timeout and the configured language. [`Cleanup::from_settings`] returns `None` when the
/// cleanup is off.
#[derive(Debug, Clone)]
pub struct Cleanup {
    provider: OpenAiCompat,
    vocabulary: Vec<String>,
    /// The configured transcription language is Spanish.
    spanish: bool,
    timeout: Duration,
    style: CleanupStyle,
    custom_prompt: String,
}

impl Cleanup {
    /// Resolves `settings.cleanup`. Returns `None` when
    /// cleanup is off or the base URL is not a loopback address.
    pub fn from_settings(settings: &Settings, client: &reqwest::Client) -> Option<Self> {
        let (name, default_url, default_model) = match settings.cleanup {
            CleanupProvider::Off => return None,
            CleanupProvider::Ollama => ("ollama", OLLAMA_URL, "llama3.2"),
            // An empty model means "whatever LM Studio has loaded" (resolved at call time).
            CleanupProvider::Lmstudio => ("lmstudio", LMSTUDIO_URL, ""),
        };
        let custom = settings.cleanup_base_url.trim().trim_end_matches('/');
        let base_url = if custom.is_empty() {
            default_url.to_string()
        } else {
            custom.to_string()
        };
        if !is_loopback_url(&base_url) {
            log::warn!("AI cleanup skipped: its server address is not on this computer");
            return None;
        }
        let model = if settings.cleanup_model.trim().is_empty() {
            default_model.to_string()
        } else {
            settings.cleanup_model.trim().to_string()
        };
        Some(Self {
            provider: OpenAiCompat {
                client: client.clone(),
                name,
                base_url,
                model,
            },
            vocabulary: settings.vocabulary.clone(),
            spanish: settings.language.starts_with("es"),
            timeout: Duration::from_secs(settings.cleanup_timeout_seconds.into()),
            // A custom style with no prompt yet behaves like the standard cleanup.
            style: if settings.cleanup_style == CleanupStyle::Custom
                && settings.cleanup_custom_prompt.trim().is_empty()
            {
                CleanupStyle::Standard
            } else {
                settings.cleanup_style
            },
            custom_prompt: settings.cleanup_custom_prompt.trim().to_string(),
        })
    }

    /// The system and user messages for `raw` in the configured style.
    fn messages(&self, raw: &str, spanish: bool) -> (String, String) {
        let system = prompt::styled_system_prompt(&self.vocabulary, spanish, self.style);
        let user = if self.style == CleanupStyle::Custom {
            prompt::custom_user_message(&self.custom_prompt, raw)
        } else {
            prompt::user_message(raw)
        };
        (system, user)
    }

    /// `ollama (llama3.2)`, for logs and the settings "Test".
    pub fn label(&self) -> String {
        format!("{} ({})", self.provider.name, self.provider.model)
    }

    /// Whether the prompt gets the Spanish line: the configured language is
    /// Spanish, or the transcriber detected Spanish.
    pub fn wants_spanish(&self, detected_language: Option<&str>) -> bool {
        self.spanish || detected_language.is_some_and(|l| l.starts_with("es"))
    }

    /// Cleans `raw`. `Ok(None)`: the guards rejected the output, use the raw text. `Err`: the
    /// server failed (or the job was cancelled); the caller keeps the raw text and tells the
    /// user why.
    pub async fn polish(
        &self,
        raw: &str,
        spanish: bool,
        cancel: CancellationToken,
    ) -> Result<Option<String>, LlmError> {
        if raw.trim().is_empty() {
            return Ok(None);
        }
        let (system, user) = self.messages(raw, spanish);
        let req = LlmRequest {
            system,
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentPart::Text(user)],
            }],
            max_tokens: 4096,
            timeout: self.timeout,
        };
        match self.provider.complete(req, cancel).await {
            Ok(r) => {
                let accepted = accept_cleanup(raw, &r.text);
                if accepted.is_none() {
                    log::warn!(
                        "cleanup output much longer than the transcript; using the raw text"
                    );
                }
                Ok(accepted.map(|text| prompt::finish(self.style, &text)))
            }
            Err(LlmError::Cancelled) => Err(LlmError::Cancelled),
            Err(e) => {
                // The reason only, never the text. Callers keep the raw text.
                log::warn!("cleanup via {} failed: {e}", self.label());
                Err(e)
            }
        }
    }
}

/// Lists the models an OpenAI-compatible local server exposes (`GET /v1/models`), for
/// "Detect" in settings and for LM Studio's loaded model. `None` if unreachable or not local.
pub async fn local_models(client: &reqwest::Client, base_url: &str) -> Option<Vec<String>> {
    if !is_loopback_url(base_url) {
        return None;
    }
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let response = client
        .get(url)
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let body = read_json_limited(response, MAX_RESPONSE_BYTES).await.ok()?;
    Some(
        body["data"]
            .as_array()?
            .iter()
            .filter_map(|m| m["id"].as_str().map(str::to_string))
            .collect(),
    )
}

/// Standard base64 (RFC 4648) for image parts.
pub(crate) fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(cleanup: CleanupProvider) -> Settings {
        Settings {
            cleanup,
            ..Settings::default()
        }
    }

    fn resolve(s: &Settings) -> Option<Cleanup> {
        Cleanup::from_settings(s, &local_client())
    }

    fn provider(s: &Settings) -> OpenAiCompat {
        resolve(s).expect("a provider").provider
    }

    #[test]
    fn off_resolves_to_none() {
        assert!(resolve(&settings(CleanupProvider::Off)).is_none());
    }

    #[test]
    fn style_shapes_the_messages() {
        let mut s = settings(CleanupProvider::Ollama);
        s.cleanup_style = CleanupStyle::VeryCasual;
        let (system, user) = resolve(&s).unwrap().messages("hi there", false);
        assert!(system.contains("Style: very casual."));
        assert_eq!(user, prompt::user_message("hi there"));

        s.cleanup_style = CleanupStyle::Custom;
        s.cleanup_custom_prompt = "  Make it a haiku: ${output} ".into();
        let (system, user) = resolve(&s).unwrap().messages("hi there", false);
        assert!(!system.contains("Style: "));
        assert_eq!(
            user,
            "Make it a haiku: <transcript>\nhi there\n</transcript>"
        );

        // Custom with nothing written yet behaves like the standard cleanup.
        s.cleanup_custom_prompt = "   ".into();
        let (system, user) = resolve(&s).unwrap().messages("hi", false);
        assert_eq!(system, prompt::INSTRUCTIONS);
        assert_eq!(user, prompt::user_message("hi"));
    }

    #[test]
    fn local_providers_default_to_localhost() {
        let ollama = provider(&settings(CleanupProvider::Ollama));
        assert_eq!(ollama.base_url, OLLAMA_URL);
        assert_eq!(ollama.model, "llama3.2");
        let lmstudio = provider(&settings(CleanupProvider::Lmstudio));
        assert_eq!(lmstudio.base_url, LMSTUDIO_URL);
        assert_eq!(lmstudio.model, "", "the loaded model is asked at call time");
    }

    #[test]
    fn custom_model_and_loopback_base_url() {
        let s = Settings {
            cleanup: CleanupProvider::Ollama,
            cleanup_model: "qwen3".into(),
            cleanup_base_url: "http://127.0.0.1:11435/v1/".into(),
            ..Settings::default()
        };
        let p = provider(&s);
        assert_eq!(p.model, "qwen3");
        assert_eq!(p.base_url, "http://127.0.0.1:11435/v1");
    }

    #[test]
    fn spanish_line_follows_configured_or_detected_language() {
        let mut s = settings(CleanupProvider::Ollama);
        let auto = resolve(&s).expect("a provider");
        assert!(!auto.wants_spanish(None));
        assert!(!auto.wants_spanish(Some("en")));
        assert!(auto.wants_spanish(Some("es")));
        s.language = "es".into();
        let es = resolve(&s).expect("a provider");
        assert!(es.wants_spanish(None));
        assert!(es.wants_spanish(Some("en")));
    }

    #[test]
    fn remote_base_url_is_never_used() {
        for url in ["http://box:11434/v1", "https://api.openai.com/v1"] {
            let s = Settings {
                cleanup: CleanupProvider::Lmstudio,
                cleanup_base_url: url.into(),
                ..Settings::default()
            };
            assert!(resolve(&s).is_none(), "{url}");
        }
    }

    #[tokio::test]
    async fn local_models_refuses_remote_hosts() {
        assert!(
            local_models(&local_client(), "https://example.com/v1")
                .await
                .is_none()
        );
    }

    /// One-shot HTTP/1.1 server on 127.0.0.1 answering every request with `body`.
    async fn serve(body: Vec<u8>) -> u16 {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                let body = body.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let _ = sock.read(&mut buf).await;
                    let head = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                                Connection: close\r\n\r\n";
                    let _ = sock.write_all(head.as_bytes()).await;
                    let _ = sock.write_all(&body).await;
                });
            }
        });
        port
    }

    #[tokio::test]
    async fn localhost_reaches_the_loopback_server() {
        let port = serve(br#"{"data":[{"id":"llama3.2"}]}"#.to_vec()).await;
        let models = local_models(&local_client(), &format!("http://localhost:{port}/v1")).await;
        assert_eq!(models, Some(vec!["llama3.2".to_string()]));
    }

    #[tokio::test]
    async fn oversized_responses_are_rejected() {
        // A valid JSON document just past the limit, without Content-Length (streamed).
        let mut body = br#"{"data":[],"pad":""#.to_vec();
        body.resize(MAX_RESPONSE_BYTES + 16, b'a');
        body.extend_from_slice(br#""}"#);
        let port = serve(body).await;
        let url = format!("http://127.0.0.1:{port}/v1");
        assert!(local_models(&local_client(), &url).await.is_none());
        let response = local_client()
            .get(format!("{url}/models"))
            .send()
            .await
            .unwrap();
        assert!(matches!(
            read_json_limited(response, MAX_RESPONSE_BYTES).await,
            Err(LlmError::BadResponse)
        ));
    }

    #[test]
    fn base64_matches_rfc4648_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }
}
