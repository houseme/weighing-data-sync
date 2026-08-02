//! WebSocket 云端客户端。
//!
//! `websocket` 特性提供一次批量上报、等待服务端 JSON ACK 的轻量客户端。
//! 主生产链路当前仍由 [`crate::sync`] 中的 HTTP 批量上报驱动；本模块用于需要
//! 长连接网关或后续切换传输层时复用同一批次语义。

use std::time::Duration;

use anyhow::{Context, anyhow, bail, ensure};
use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::time::timeout;
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{
        client::IntoClientRequest,
        http::header::{AUTHORIZATION, CONTENT_TYPE},
        protocol::{Message, WebSocketConfig},
    },
};

const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_ACK_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_WRITE_BUFFER_BYTES: usize = 128 * 1024;

/// WebSocket 批量上报客户端。
///
/// 客户端本身不持有连接状态，因此可以安全 clone 并在多个同步任务中独立使用。
/// 每次 [`push_batch`](Self::push_batch) 都会建立连接、发送一批 JSON、等待服务端 ACK，
/// 适合当前“一批记录一个确认”的同步语义。
#[derive(Debug, Clone)]
pub struct WebSocketClient {
    endpoint: String,
    api_key: Option<String>,
    connect_timeout: Duration,
    ack_timeout: Duration,
    max_message_bytes: usize,
    write_buffer_bytes: usize,
}

/// 服务端批量确认结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebSocketAck {
    pub accepted: bool,
    pub received: Option<usize>,
    pub message: Option<String>,
}

#[derive(Debug, Serialize)]
struct WebSocketUploadRequest<'a> {
    source: &'static str,
    uploaded_at: DateTime<Utc>,
    records: &'a [Value],
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct AckEnvelope {
    accepted: Option<bool>,
    ok: Option<bool>,
    success: Option<bool>,
    status: Option<String>,
    received: Option<usize>,
    message: Option<String>,
    failed: Option<usize>,
    failed_ids: Vec<Value>,
}

impl WebSocketClient {
    /// 创建一个指向 `endpoint` 的 WebSocket 客户端。
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            api_key: None,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            ack_timeout: DEFAULT_ACK_TIMEOUT,
            max_message_bytes: DEFAULT_MAX_MESSAGE_BYTES,
            write_buffer_bytes: DEFAULT_WRITE_BUFFER_BYTES,
        }
    }

    /// 设置可选 Bearer Token。
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// 设置连接建立超时与 ACK 等待超时。
    pub fn with_timeouts(mut self, connect_timeout: Duration, ack_timeout: Duration) -> Self {
        self.connect_timeout = connect_timeout;
        self.ack_timeout = ack_timeout;
        self
    }

    /// 设置单批上报和服务端 ACK 的最大消息大小。
    pub fn with_max_message_bytes(mut self, max_message_bytes: usize) -> Self {
        self.max_message_bytes = max_message_bytes;
        self
    }

    /// 设置 tungstenite 写缓冲触发阈值。
    pub fn with_write_buffer_bytes(mut self, write_buffer_bytes: usize) -> Self {
        self.write_buffer_bytes = write_buffer_bytes;
        self
    }

    /// 已配置的 WebSocket 端点。
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// 推送一批记录并要求服务端返回成功 ACK。
    pub async fn push_batch(&self, records: &[Value]) -> anyhow::Result<()> {
        self.push_batch_with_ack(records).await.map(|_| ())
    }

    /// 推送一批记录并返回服务端 ACK。
    pub async fn push_batch_with_ack(&self, records: &[Value]) -> anyhow::Result<WebSocketAck> {
        let payload = WebSocketUploadRequest {
            source: "weighing-data-sync",
            uploaded_at: Utc::now(),
            records,
        };
        let payload =
            serde_json::to_vec(&payload).context("failed to serialize websocket batch")?;
        ensure!(
            payload.len() <= self.max_message_bytes,
            "websocket batch payload is {} bytes, exceeding configured max {} bytes",
            payload.len(),
            self.max_message_bytes
        );

        let request = self.build_request()?;
        let config = self.websocket_config()?;
        let connect = connect_async_with_config(request, Some(config), true);
        let (mut socket, _) = timeout(self.connect_timeout, connect)
            .await
            .context("websocket connect timed out")?
            .context("failed to connect websocket endpoint")?;

        timeout(self.ack_timeout, socket.send(Message::binary(payload)))
            .await
            .context("websocket send timed out")?
            .context("failed to send websocket batch")?;

        let ack = self.read_ack(&mut socket).await;
        let _ = timeout(Duration::from_millis(250), socket.close(None)).await;
        let ack = ack?;
        validate_ack_record_count(&ack, records.len())?;
        Ok(ack)
    }

    fn build_request(
        &self,
    ) -> anyhow::Result<tokio_tungstenite::tungstenite::handshake::client::Request> {
        let mut request = self
            .endpoint
            .as_str()
            .into_client_request()
            .with_context(|| format!("invalid websocket endpoint {}", self.endpoint))?;
        request
            .headers_mut()
            .insert(CONTENT_TYPE, "application/json".parse()?);

        if let Some(api_key) = &self.api_key {
            let value = format!("Bearer {api_key}");
            request.headers_mut().insert(AUTHORIZATION, value.parse()?);
        }

        Ok(request)
    }

    fn websocket_config(&self) -> anyhow::Result<WebSocketConfig> {
        let max_write_buffer_size = self
            .write_buffer_bytes
            .checked_add(self.max_message_bytes.max(1))
            .context("websocket write buffer and max message size are too large")?;
        let max_write_buffer_size =
            max_write_buffer_size.max(self.write_buffer_bytes.saturating_add(1));

        Ok(WebSocketConfig::default()
            .write_buffer_size(self.write_buffer_bytes)
            .max_write_buffer_size(max_write_buffer_size)
            .max_message_size(Some(self.max_message_bytes))
            .max_frame_size(Some(self.max_message_bytes)))
    }

    async fn read_ack<S>(&self, socket: &mut S) -> anyhow::Result<WebSocketAck>
    where
        S: futures_util::Sink<Message, Error = tokio_tungstenite::tungstenite::Error>
            + futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
            + Unpin,
    {
        loop {
            let frame = timeout(self.ack_timeout, socket.next())
                .await
                .context("websocket ack timed out")?
                .ok_or_else(|| anyhow!("websocket closed before ack"))?
                .context("failed to read websocket ack")?;

            match frame {
                Message::Text(text) => return parse_ack_payload(text.as_bytes()),
                Message::Binary(bytes) => return parse_ack_payload(&bytes),
                Message::Ping(payload) => {
                    timeout(self.ack_timeout, socket.send(Message::Pong(payload)))
                        .await
                        .context("websocket pong timed out")?
                        .context("failed to send websocket pong")?;
                }
                Message::Pong(_) => {}
                Message::Close(frame) => {
                    let reason = frame
                        .map(|frame| frame.reason.to_string())
                        .filter(|reason| !reason.is_empty())
                        .unwrap_or_else(|| "remote closed websocket before ack".to_owned());
                    bail!("{reason}");
                }
                Message::Frame(_) => {}
            }
        }
    }
}

fn parse_ack_payload(payload: &[u8]) -> anyhow::Result<WebSocketAck> {
    let value: Value = serde_json::from_slice(payload).context("failed to decode websocket ack")?;
    match value {
        Value::Bool(true) => Ok(WebSocketAck {
            accepted: true,
            received: None,
            message: None,
        }),
        Value::Bool(false) => bail!("websocket endpoint rejected batch"),
        Value::String(status) => ack_from_status(status, None, None),
        Value::Object(_) => {
            let envelope: AckEnvelope =
                serde_json::from_value(value).context("failed to parse websocket ack")?;
            ack_from_envelope(envelope)
        }
        other => bail!("unsupported websocket ack payload: {other}"),
    }
}

fn ack_from_envelope(envelope: AckEnvelope) -> anyhow::Result<WebSocketAck> {
    if envelope.failed.unwrap_or_default() > 0 || !envelope.failed_ids.is_empty() {
        bail!(
            "websocket endpoint rejected {} records",
            envelope.failed.unwrap_or(envelope.failed_ids.len())
        );
    }

    let accepted = envelope
        .accepted
        .or(envelope.ok)
        .or(envelope.success)
        .or_else(|| status_is_success(envelope.status.as_deref()));

    match accepted {
        Some(true) => Ok(WebSocketAck {
            accepted: true,
            received: envelope.received,
            message: envelope.message,
        }),
        Some(false) => bail!(
            "{}",
            envelope
                .message
                .unwrap_or_else(|| "websocket endpoint rejected batch".to_owned())
        ),
        None => bail!("websocket ack must contain accepted/ok/success or a success status"),
    }
}

fn validate_ack_record_count(ack: &WebSocketAck, expected: usize) -> anyhow::Result<()> {
    if let Some(received) = ack.received {
        ensure!(
            received == expected,
            "websocket ack received count {received} does not match sent count {expected}"
        );
    }
    Ok(())
}

fn ack_from_status(
    status: String,
    received: Option<usize>,
    message: Option<String>,
) -> anyhow::Result<WebSocketAck> {
    match status_is_success(Some(status.as_str())) {
        Some(true) => Ok(WebSocketAck {
            accepted: true,
            received,
            message,
        }),
        Some(false) => bail!("websocket endpoint rejected batch with status {status}"),
        None => bail!("unsupported websocket ack status {status}"),
    }
}

fn status_is_success(status: Option<&str>) -> Option<bool> {
    let status = status?.trim();
    if status.eq_ignore_ascii_case("ok")
        || status.eq_ignore_ascii_case("ack")
        || status.eq_ignore_ascii_case("accepted")
        || status.eq_ignore_ascii_case("success")
        || status.eq_ignore_ascii_case("succeeded")
    {
        Some(true)
    } else if status.eq_ignore_ascii_case("error")
        || status.eq_ignore_ascii_case("failed")
        || status.eq_ignore_ascii_case("rejected")
    {
        Some(false)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use futures_util::{SinkExt, StreamExt};
    use serde_json::json;
    use tokio::net::TcpListener;
    use tokio_tungstenite::{accept_async, tungstenite::protocol::Message};

    use super::*;

    #[test]
    fn parse_ack_accepts_common_success_shapes() {
        assert!(parse_ack_payload(br#"true"#).unwrap().accepted);
        assert_eq!(
            parse_ack_payload(br#"{"ok":true,"received":2}"#)
                .unwrap()
                .received,
            Some(2)
        );
        assert!(parse_ack_payload(br#"{"status":"accepted"}"#).is_ok());
        assert!(parse_ack_payload(br#""success""#).is_ok());
    }

    #[test]
    fn parse_ack_rejects_failures_and_ambiguous_payloads() {
        assert!(parse_ack_payload(br#"false"#).is_err());
        assert!(parse_ack_payload(br#"{"ok":false,"message":"bad batch"}"#).is_err());
        assert!(parse_ack_payload(br#"{"received":2}"#).is_err());
        assert!(parse_ack_payload(br#"{"ok":true,"failed":1}"#).is_err());
    }

    #[test]
    fn validates_ack_received_count_when_present() {
        let ack = WebSocketAck {
            accepted: true,
            received: Some(2),
            message: None,
        };

        assert!(validate_ack_record_count(&ack, 2).is_ok());
        assert!(validate_ack_record_count(&ack, 1).is_err());
    }

    #[test]
    fn websocket_config_rejects_impossible_buffer_sizes() {
        let error = WebSocketClient::new("ws://127.0.0.1:9")
            .with_write_buffer_bytes(usize::MAX)
            .websocket_config()
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("write buffer and max message size are too large")
        );
    }

    #[tokio::test]
    async fn push_batch_sends_json_and_reads_ack() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("ws://{}", listener.local_addr().unwrap());

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            let message = socket.next().await.unwrap().unwrap();
            let payload: Value = serde_json::from_slice(&message.into_data()).unwrap();

            assert_eq!(payload["source"], "weighing-data-sync");
            assert_eq!(payload["records"].as_array().unwrap().len(), 2);

            socket
                .send(Message::text(
                    json!({"accepted": true, "received": 2}).to_string(),
                ))
                .await
                .unwrap();
        });

        let records = vec![json!({"ticket_no": "T1"}), json!({"ticket_no": "T2"})];
        let ack = WebSocketClient::new(endpoint)
            .with_timeouts(Duration::from_secs(2), Duration::from_secs(2))
            .push_batch_with_ack(&records)
            .await
            .unwrap();

        assert_eq!(
            ack,
            WebSocketAck {
                accepted: true,
                received: Some(2),
                message: None,
            }
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn push_batch_enforces_payload_limit_before_connecting() {
        let records = vec![json!({"ticket_no": "T1", "payload": "too-large"})];
        let error = WebSocketClient::new("ws://127.0.0.1:9")
            .with_max_message_bytes(8)
            .push_batch(&records)
            .await
            .unwrap_err();

        assert!(error.to_string().contains("exceeding configured max"));
    }
}
