use crate::error::{AsrError, Result};
use base64::Engine;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::http::Request;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type WsResponse = tokio_tungstenite::tungstenite::http::Response<Option<Vec<u8>>>;

/// Connect WebSocket, using proxy if configured, falling back to env vars.
pub async fn connect_ws(
    request: Request<()>,
    proxy_url: &str,
    proxy_username: &str,
    proxy_password: &str,
) -> Result<(WsStream, WsResponse)> {
    let effective_proxy = if !proxy_url.is_empty() {
        Some(proxy_url.to_string())
    } else if let Ok(v) = std::env::var("HTTPS_PROXY") {
        Some(v)
    } else if let Ok(v) = std::env::var("HTTP_PROXY") {
        Some(v)
    } else {
        None
    };

    match effective_proxy {
        Some(ref url) => {
            connect_via_proxy(request, url, proxy_username, proxy_password).await
        }
        None => {
            connect_async(request)
                .await
                .map_err(|e| AsrError::Connection(e.to_string()))
        }
    }
}

/// Establish HTTP CONNECT tunnel, then perform WebSocket handshake over it.
async fn connect_via_proxy(
    request: Request<()>,
    proxy_url: &str,
    username: &str,
    password: &str,
) -> Result<(WsStream, WsResponse)> {
    let proxy_uri: url::Url = proxy_url
        .parse()
        .map_err(|e| AsrError::Connection(format!("invalid proxy URL: {e}")))?;

    let proxy_host = proxy_uri
        .host_str()
        .ok_or_else(|| AsrError::Connection("proxy URL missing host".into()))?;
    let proxy_port = proxy_uri.port().unwrap_or(8080);

    let target_uri = request.uri();
    let target_host = target_uri
        .host()
        .ok_or_else(|| AsrError::Connection("request URL missing host".into()))?;
    let target_port = target_uri.port_u16().unwrap_or(if target_uri.scheme_str() == Some("wss") { 443 } else { 80 });

    let mut stream = TcpStream::connect(format!("{proxy_host}:{proxy_port}"))
        .await
        .map_err(|e| AsrError::Connection(format!("proxy connect failed: {e}")))?;

    let mut connect_req = format!(
        "CONNECT {target_host}:{target_port} HTTP/1.1\r\nHost: {target_host}:{target_port}\r\n"
    );

    if !username.is_empty() {
        let cred = base64::engine::general_purpose::STANDARD
            .encode(format!("{username}:{password}"));
        connect_req.push_str(&format!("Proxy-Authorization: Basic {cred}\r\n"));
    }
    connect_req.push_str("\r\n");

    stream
        .write_all(connect_req.as_bytes())
        .await
        .map_err(|e| AsrError::Connection(format!("proxy write failed: {e}")))?;

    // Read response until \r\n\r\n
    let mut buf = Vec::with_capacity(512);
    loop {
        let byte = stream
            .read_u8()
            .await
            .map_err(|e| AsrError::Connection(format!("proxy read failed: {e}")))?;
        buf.push(byte);
        if buf.len() >= 4 && &buf[buf.len() - 4..] == b"\r\n\r\n" {
            break;
        }
        if buf.len() > 4096 {
            return Err(AsrError::Connection("proxy response too large".into()));
        }
    }

    let response_str = String::from_utf8_lossy(&buf);
    if !response_str.contains("200") {
        return Err(AsrError::Connection(format!(
            "proxy CONNECT failed: {}",
            response_str.lines().next().unwrap_or("unknown")
        )));
    }

    log::info!("proxy tunnel established to {target_host}:{target_port}");

    // WebSocket + TLS handshake over the tunneled stream
    tokio_tungstenite::client_async_tls_with_config(request, stream, None, None)
        .await
        .map_err(|e| AsrError::Connection(format!("WebSocket handshake via proxy failed: {e}")))
}
