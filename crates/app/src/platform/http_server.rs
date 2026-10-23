//! A minimal HTTP/1.1 server on the loopback interface, used by the MCP endpoint (desktop only).
//! Requests need a Content-Length (no chunked bodies); headers and bodies are bounded; connections are
//! kept alive until the client closes them or stays idle for two minutes.

use std::future::Future;
use std::net::{Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

const MAX_HEAD_BYTES: usize = 16 * 1024;
const IDLE_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub method: String,
    /// Path without the query string.
    pub path: String,
    /// Lowercase names.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn empty(status: u16) -> Self {
        Self { status, headers: Vec::new(), body: Vec::new() }
    }

    pub fn json(status: u16, body: String) -> Self {
        Self { status, headers: vec![("Content-Type".into(), "application/json".into())], body: body.into_bytes() }
    }

    pub fn with_header(mut self, name: &str, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}

pub type Handler = Arc<dyn Fn(Request) -> Pin<Box<dyn Future<Output = Response> + Send>> + Send + Sync>;

/// Stops listening when dropped. Open connections end after their current request.
pub struct Server {
    pub port: u16,
    stop: watch::Sender<bool>,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
    }
}

/// Binds `127.0.0.1:port` and serves requests with `handler`. Fails when the port is taken.
pub fn start(port: u16, max_body: usize, handler: Handler) -> std::io::Result<Server> {
    let listener = std::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    let runtime = super::runtime();
    let listener = {
        let _guard = runtime.enter();
        TcpListener::from_std(listener)?
    };
    let (stop, mut stopped) = watch::channel(false);
    runtime.spawn(async move {
        loop {
            tokio::select! {
                _ = stopped.changed() => break,
                accepted = listener.accept() => match accepted {
                    Ok((stream, _)) => {
                        let handler = handler.clone();
                        let stopped = stopped.clone();
                        tokio::spawn(serve(stream, handler, max_body, stopped));
                    }
                    Err(error) => {
                        log::warn!("MCP connection failed: {error}");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                },
            }
        }
    });
    Ok(Server { port, stop })
}

async fn serve(mut stream: TcpStream, handler: Handler, max_body: usize, stopped: watch::Receiver<bool>) {
    let mut buffer = Vec::new();
    loop {
        if *stopped.borrow() {
            return;
        }
        let request = match tokio::time::timeout(IDLE_TIMEOUT, read_request(&mut stream, &mut buffer, max_body)).await {
            Err(_) | Ok(Ok(None)) => return,
            Ok(Err(status)) => {
                let _ = write_response(&mut stream, &Response::empty(status), true).await;
                return;
            }
            Ok(Ok(Some(request))) => request,
        };
        let close = request.header("connection").is_some_and(|c| c.eq_ignore_ascii_case("close"));
        let response = handler(request).await;
        if write_response(&mut stream, &response, close).await.is_err() || close {
            return;
        }
    }
}

/// Reads one request. `Ok(None)` when the client closed the connection between requests.
async fn read_request(stream: &mut TcpStream, buffer: &mut Vec<u8>, max_body: usize) -> Result<Option<Request>, u16> {
    let head_end = loop {
        if let Some(end) = find_head_end(buffer) {
            break end;
        }
        if buffer.len() > MAX_HEAD_BYTES {
            return Err(431);
        }
        let mut chunk = [0u8; 4096];
        match stream.read(&mut chunk).await {
            Ok(0) if buffer.is_empty() => return Ok(None),
            Ok(0) | Err(_) => return Err(400),
            Ok(n) => buffer.extend_from_slice(&chunk[..n]),
        }
    };
    let (method, path, headers) = parse_head(&buffer[..head_end])?;
    if headers.iter().any(|(n, _)| n == "transfer-encoding") {
        return Err(411);
    }
    let length = match headers.iter().find(|(n, _)| n == "content-length") {
        Some((_, value)) => value.trim().parse::<usize>().map_err(|_| 400u16)?,
        None => 0,
    };
    if length > max_body {
        return Err(413);
    }
    let body_start = head_end + 4;
    while buffer.len() < body_start + length {
        let mut chunk = vec![0u8; (body_start + length - buffer.len()).min(64 * 1024)];
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => return Err(400),
            Ok(n) => buffer.extend_from_slice(&chunk[..n]),
        }
    }
    let body = buffer[body_start..body_start + length].to_vec();
    buffer.drain(..body_start + length);
    Ok(Some(Request { method, path, headers, body }))
}

fn find_head_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|w| w == b"\r\n\r\n")
}

/// Method, path and headers.
type Head = (String, String, Vec<(String, String)>);

/// The request line and headers. Header names are lowercased.
pub fn parse_head(head: &[u8]) -> Result<Head, u16> {
    let text = std::str::from_utf8(head).map_err(|_| 400u16)?;
    let mut lines = text.split("\r\n");
    let mut parts = lines.next().unwrap_or_default().split(' ');
    let (Some(method), Some(target), Some(version)) = (parts.next(), parts.next(), parts.next()) else {
        return Err(400);
    };
    if !version.starts_with("HTTP/1.") || method.is_empty() || !target.starts_with('/') {
        return Err(400);
    }
    let path = target.split(['?', '#']).next().unwrap_or_default().to_string();
    let mut headers = Vec::new();
    for line in lines {
        let (name, value) = line.split_once(':').ok_or(400u16)?;
        if name.is_empty() || name.contains(char::is_whitespace) {
            return Err(400);
        }
        headers.push((name.to_ascii_lowercase(), value.trim().to_string()));
    }
    Ok((method.to_string(), path, headers))
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        411 => "Length Required",
        413 => "Content Too Large",
        415 => "Unsupported Media Type",
        431 => "Request Header Fields Too Large",
        _ => "Error",
    }
}

async fn write_response(stream: &mut TcpStream, response: &Response, close: bool) -> std::io::Result<()> {
    let mut head = format!("HTTP/1.1 {} {}\r\nContent-Length: {}\r\n", response.status, reason(response.status), response.body.len());
    for (name, value) in &response.headers {
        if !value.contains(['\r', '\n']) {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
    }
    head.push_str("Cache-Control: no-store\r\n");
    if close {
        head.push_str("Connection: close\r\n");
    }
    head.push_str("\r\n");
    let mut bytes = head.into_bytes();
    bytes.extend_from_slice(&response.body);
    stream.write_all(&bytes).await?;
    stream.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_request_heads() {
        let (method, path, headers) =
            parse_head(b"POST /mcp?x=1 HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer abc\r\nContent-Length: 2").unwrap();
        assert_eq!((method.as_str(), path.as_str()), ("POST", "/mcp"));
        assert_eq!(headers[1], ("authorization".to_string(), "Bearer abc".to_string()));
        assert_eq!(parse_head(b"GARBAGE"), Err(400));
        assert_eq!(parse_head(b"GET http://evil/ HTTP/1.1"), Err(400));
        assert_eq!(parse_head(b"GET / HTTP/1.1\r\nBad Header: x"), Err(400));
    }

    #[test]
    fn serves_keep_alive_requests_and_rejects_large_bodies() {
        let handler: Handler = Arc::new(|request: Request| {
            Box::pin(async move { Response::json(200, String::from_utf8(request.body).unwrap()) })
        });
        let server = start(0, 16, handler).unwrap();
        let port = server.port;
        super::super::runtime().block_on(async move {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            for body in ["{}", "[1]"] {
                let request = format!("POST /mcp HTTP/1.1\r\nContent-Length: {}\r\n\r\n{body}", body.len());
                stream.write_all(request.as_bytes()).await.unwrap();
                // A reply can arrive in several reads.
                let mut reply = Vec::new();
                while !reply.ends_with(body.as_bytes()) {
                    let mut chunk = [0u8; 256];
                    let n = stream.read(&mut chunk).await.unwrap();
                    assert!(n > 0, "connection closed early");
                    reply.extend_from_slice(&chunk[..n]);
                }
                let text = String::from_utf8_lossy(&reply).to_string();
                assert!(text.starts_with("HTTP/1.1 200 OK") && text.ends_with(body), "{text}");
            }
            let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            stream.write_all(b"POST /mcp HTTP/1.1\r\nContent-Length: 999\r\n\r\n").await.unwrap();
            let mut reply = String::new();
            stream.read_to_string(&mut reply).await.unwrap();
            assert!(reply.starts_with("HTTP/1.1 413"), "{reply}");
        });
        drop(server);
    }
}
