//! The "Local model" provider: an OpenAI-compatible chat completions server such as Ollama, LM Studio or
//! llama.cpp. Requests stream, use a strict JSON schema and greedy sampling, and never leave the
//! configured address.

use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use url::Url;

use super::edits::AssistantError;
use super::music_assistant::AssistantSchema;

pub const DEFAULT_BASE_URL: &str = "http://localhost:11434/v1";
/// Response token budget, as for the on-device model on iOS.
pub const MAX_RESPONSE_TOKENS: u32 = 1_000;
const MAX_RESPONSE_BYTES: usize = 2_000_000;

/// What the person configures for the local provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalModelSettings {
    #[serde(default = "default_base")]
    pub base_url: String,
    #[serde(default)]
    pub model: String,
    /// Optional bearer token for servers that require one. The app stores it with other secrets.
    #[serde(default, skip_serializing)]
    pub api_key: Option<String>,
}

fn default_base() -> String {
    DEFAULT_BASE_URL.to_string()
}

impl Default for LocalModelSettings {
    fn default() -> Self {
        Self {
            base_url: default_base(),
            model: String::new(),
            api_key: None,
        }
    }
}

impl LocalModelSettings {
    /// The base URL with a trailing slash, accepting only http(s) without credentials, query or fragment.
    pub fn base(&self) -> Result<Url, AssistantError> {
        let mut url =
            Url::parse(self.base_url.trim()).map_err(|_| AssistantError::LocalUnavailable)?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none_or(str::is_empty)
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(AssistantError::LocalUnavailable);
        }
        if !url.path().ends_with('/') {
            let path = format!("{}/", url.path());
            url.set_path(&path);
        }
        Ok(url)
    }
}

#[derive(Debug, Clone)]
pub struct LocalModelClient {
    http: reqwest::Client,
    base: Url,
    model: String,
    api_key: Option<String>,
}

impl LocalModelClient {
    pub fn new(settings: &LocalModelSettings) -> Result<Self, AssistantError> {
        Self::with_http(crate::http::server_client(), settings)
    }

    pub fn with_http(
        http: reqwest::Client,
        settings: &LocalModelSettings,
    ) -> Result<Self, AssistantError> {
        Ok(Self {
            http,
            base: settings.base()?,
            model: settings.model.trim().to_string(),
            api_key: settings.api_key.clone().filter(|k| !k.is_empty()),
        })
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    fn request(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.api_key {
            Some(key) => builder.bearer_auth(key),
            None => builder,
        }
    }

    /// Model IDs offered by the server (`GET /models`), sorted.
    pub async fn models(&self) -> Result<Vec<String>, AssistantError> {
        #[derive(Deserialize)]
        struct Entry {
            id: String,
        }
        #[derive(Deserialize)]
        struct List {
            data: Vec<Entry>,
        }
        let url = self
            .base
            .join("models")
            .map_err(|_| AssistantError::LocalUnavailable)?;
        let response = self
            .request(self.http.get(url))
            .send()
            .await
            .map_err(|_| AssistantError::LocalUnavailable)?;
        if !response.status().is_success() {
            return Err(AssistantError::LocalUnavailable);
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|_| AssistantError::LocalUnavailable)?;
        if bytes.len() > MAX_RESPONSE_BYTES {
            return Err(AssistantError::InvalidResponse);
        }
        let list: List =
            serde_json::from_slice(&bytes).map_err(|_| AssistantError::InvalidResponse)?;
        let mut ids: Vec<String> = list
            .data
            .into_iter()
            .map(|e| e.id)
            .filter(|id| !id.is_empty())
            .collect();
        ids.sort();
        ids.dedup();
        Ok(ids)
    }

    pub fn request_body(
        model: &str,
        instructions: &str,
        input: &str,
        schema: &AssistantSchema,
    ) -> Value {
        json!({
            "model": model,
            "messages": [{"role": "system", "content": instructions}, {"role": "user", "content": input}],
            "stream": true,
            "temperature": 0,
            "max_tokens": MAX_RESPONSE_TOKENS,
            "response_format": {"type": "json_schema", "json_schema": {"name": "discophile_music", "strict": true, "schema": schema.json()}},
        })
    }

    /// Streams one structured response and returns its JSON text.
    pub async fn respond(
        &self,
        instructions: &str,
        input: &str,
        schema: &AssistantSchema,
    ) -> Result<String, AssistantError> {
        if self.model.is_empty() {
            return Err(AssistantError::LocalUnavailable);
        }
        if let AssistantSchema::Selection { ids, .. } = schema
            && ids.is_empty()
        {
            return Err(AssistantError::NoMatches);
        }
        let url = self
            .base
            .join("chat/completions")
            .map_err(|_| AssistantError::LocalUnavailable)?;
        let response = self
            .request(self.http.post(url))
            .header("Accept", "text/event-stream")
            .json(&Self::request_body(
                &self.model,
                instructions,
                input,
                schema,
            ))
            .send()
            .await
            .map_err(|_| AssistantError::LocalUnavailable)?;
        let status = response.status();
        if status.as_u16() == 400 {
            return Err(AssistantError::InvalidResponse);
        }
        if !status.is_success() {
            return Err(AssistantError::LocalUnavailable);
        }
        let mut body = response.bytes_stream();
        let mut stream = ChatCompletionStream::default();
        while let Some(chunk) = body.next().await {
            stream.append(&chunk.map_err(|_| AssistantError::LocalUnavailable)?)?;
        }
        stream.finish()
    }
}

/// Incremental decoding of chat completion chunks, also accepting a single non-streamed JSON reply.
#[derive(Debug, Clone, Default)]
pub struct ChatCompletionStream {
    pending: Vec<u8>,
    raw: Vec<u8>,
    text: String,
    finished: bool,
    streamed: bool,
}

impl ChatCompletionStream {
    pub fn append(&mut self, data: &[u8]) -> Result<(), AssistantError> {
        if self.raw.len() + data.len() > MAX_RESPONSE_BYTES {
            return Err(AssistantError::InvalidResponse);
        }
        self.raw.extend_from_slice(data);
        self.pending.extend_from_slice(data);
        while let Some(newline) = self.pending.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=newline).collect();
            let line = String::from_utf8_lossy(&line);
            let Some(payload) = line.trim().strip_prefix("data:") else {
                continue;
            };
            self.streamed = true;
            self.event(payload.trim())?;
        }
        Ok(())
    }

    fn event(&mut self, payload: &str) -> Result<(), AssistantError> {
        if payload == "[DONE]" {
            self.finished = true;
            return Ok(());
        }
        let value: Value =
            serde_json::from_str(payload).map_err(|_| AssistantError::InvalidResponse)?;
        self.choice(&value, "delta")
    }

    fn choice(&mut self, value: &Value, key: &str) -> Result<(), AssistantError> {
        if value.get("error").is_some() {
            return Err(AssistantError::LocalUnavailable);
        }
        let Some(choice) = value.get("choices").and_then(|c| c.get(0)) else {
            return Ok(());
        };
        if let Some(part) = choice
            .get(key)
            .and_then(|d| d.get("content"))
            .and_then(Value::as_str)
        {
            self.text += part;
        }
        if choice
            .get(key)
            .and_then(|d| d.get("refusal"))
            .and_then(Value::as_str)
            .is_some_and(|r| !r.is_empty())
        {
            return Err(AssistantError::Refused);
        }
        match choice.get("finish_reason").and_then(Value::as_str) {
            Some("length") => Err(AssistantError::Incomplete),
            Some("content_filter") => Err(AssistantError::Refused),
            Some(_) => {
                self.finished = true;
                Ok(())
            }
            None => Ok(()),
        }
    }

    pub fn finish(mut self) -> Result<String, AssistantError> {
        if !self.streamed {
            // The server ignored `stream` and answered with one completion object.
            let value: Value =
                serde_json::from_slice(&self.raw).map_err(|_| AssistantError::InvalidResponse)?;
            self.choice(&value, "message")?;
        } else if !self.pending.is_empty() {
            let rest = String::from_utf8_lossy(&std::mem::take(&mut self.pending)).into_owned();
            if let Some(payload) = rest.trim().strip_prefix("data:") {
                self.event(payload.trim())?;
            }
        }
        if !self.finished {
            return Err(AssistantError::Incomplete);
        }
        extract_json(&self.text).ok_or(AssistantError::InvalidResponse)
    }
}

/// The JSON object in a reply, tolerating Markdown fences, reasoning blocks or text around it.
pub fn extract_json(text: &str) -> Option<String> {
    let text = match text.rfind("</think>") {
        Some(end) => &text[end + "</think>".len()..],
        None => text,
    };
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    let candidate = text.get(start..=end)?;
    serde_json::from_str::<Value>(candidate)
        .ok()
        .filter(Value::is_object)
        .map(|_| candidate.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assistant::chatgpt::tests::serve;

    #[test]
    fn settings_validate_the_base_url() {
        let settings = LocalModelSettings::default();
        assert_eq!(
            settings.base().unwrap().as_str(),
            "http://localhost:11434/v1/"
        );
        for bad in [
            "ftp://x",
            "http://user:pw@host/v1",
            "http://host/v1?x=1",
            "not a url",
        ] {
            assert!(
                LocalModelSettings {
                    base_url: bad.into(),
                    ..Default::default()
                }
                .base()
                .is_err(),
                "{bad}"
            );
        }
        let json = serde_json::to_string(&LocalModelSettings {
            api_key: Some("secret".into()),
            ..Default::default()
        })
        .unwrap();
        assert!(!json.contains("secret"));
    }

    #[test]
    fn stream_collects_deltas_and_requires_completion() {
        let mut stream = ChatCompletionStream::default();
        stream
            .append(b"data: {\"choices\":[{\"delta\":{\"content\":\"{\\\"title\\\":\"}}]}\n\n")
            .unwrap();
        stream.append(b"data: {\"choices\":[{\"delta\":{\"content\":\"\\\"Mix\\\"}\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n").unwrap();
        assert_eq!(stream.finish().unwrap(), r#"{"title":"Mix"}"#);
        let mut partial = ChatCompletionStream::default();
        partial
            .append(b"data: {\"choices\":[{\"delta\":{\"content\":\"{}\"}}]}\n\n")
            .unwrap();
        assert_eq!(partial.finish().unwrap_err(), AssistantError::Incomplete);
        let mut truncated = ChatCompletionStream::default();
        assert_eq!(
            truncated
                .append(b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\n")
                .unwrap_err(),
            AssistantError::Incomplete
        );
        let mut whole = ChatCompletionStream::default();
        whole.append(br#"{"choices":[{"message":{"content":"```json\n{\"a\":1}\n```"},"finish_reason":"stop"}]}"#).unwrap();
        assert_eq!(whole.finish().unwrap(), r#"{"a":1}"#);
    }

    #[test]
    fn json_extraction_skips_reasoning_and_fences() {
        assert_eq!(
            extract_json("<think>{no}</think>\n```json\n{\"x\": 2}\n```").as_deref(),
            Some("{\"x\": 2}")
        );
        assert!(extract_json("no json").is_none());
    }

    #[tokio::test]
    async fn client_sends_schema_and_reads_streamed_reply() {
        let (base, log) = serve(|request| {
            if request.path.ends_with("/models") {
                return (200, br#"{"object":"list","data":[{"id":"qwen3"},{"id":"llama3.2"}]}"#.to_vec());
            }
            let body = "data: {\"choices\":[{\"delta\":{\"content\":\"{\\\"title\\\":\\\"T\\\",\\\"message\\\":\\\"\\\",\\\"track_ids\\\":[\\\"t0\\\"]}\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
            (200, body.as_bytes().to_vec())
        })
        .await;
        let settings = LocalModelSettings {
            base_url: format!("{base}v1"),
            model: "qwen3".into(),
            api_key: None,
        };
        let client = LocalModelClient::with_http(reqwest::Client::new(), &settings).unwrap();
        assert_eq!(client.models().await.unwrap(), ["llama3.2", "qwen3"]);
        let text = client
            .respond("Instructions", "Input", &AssistantSchema::selection(2, 1))
            .await
            .unwrap();
        assert_eq!(text, r#"{"title":"T","message":"","track_ids":["t0"]}"#);
        let request = log.lock().unwrap()[1].clone();
        assert_eq!(request.path, "/v1/chat/completions");
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(body["model"], "qwen3");
        assert_eq!(body["response_format"]["type"], "json_schema");
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["temperature"], 0);
        let empty = LocalModelClient::with_http(
            reqwest::Client::new(),
            &LocalModelSettings {
                model: String::new(),
                ..settings
            },
        )
        .unwrap();
        assert_eq!(
            empty
                .respond("i", "x", &AssistantSchema::Plan)
                .await
                .unwrap_err(),
            AssistantError::LocalUnavailable
        );
    }

    #[tokio::test]
    async fn unreachable_server_reports_local_unavailable() {
        let settings = LocalModelSettings {
            base_url: "http://127.0.0.1:9/v1".into(),
            model: "m".into(),
            api_key: None,
        };
        let client = LocalModelClient::with_http(reqwest::Client::new(), &settings).unwrap();
        assert_eq!(
            client
                .respond("i", "x", &AssistantSchema::Plan)
                .await
                .unwrap_err(),
            AssistantError::LocalUnavailable
        );
    }
}
