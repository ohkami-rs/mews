#![cfg(feature="__io__")]

use crate::message::{CloseFrame, CloseCode};
use crate::connection::{UnderlyingConnection, Connection};

/// **note** : currently, subprotocols via `Sec-WebSocket-Protocol` is not supported.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub write_buffer_size:      usize,
    pub max_write_buffer_size:  usize,
    pub accept_unmasked_frames: bool,
    pub max_message_size:       Option<usize>,
    pub max_frame_size:         Option<usize>,
}
const _: () = {
    impl Default for Config {
        fn default() -> Self {
            Self {
                write_buffer_size:      128 * 1024, // 128 KiB
                max_write_buffer_size:  usize::MAX,
                accept_unmasked_frames: false,
                max_message_size:       Some(64 << 20),
                max_frame_size:         Some(16 << 20),
            }
        }
    }
};

pub type Handler<C> = Box<dyn
    FnOnce(Connection<C>) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'static>>
    + Send + Sync
>;

/// *example.rs*
/// ```
/// # use tokio::time::{sleep, Duration};
/// # use tokio::{spawn, net::TcpStream};
/// # type Headers = std::collections::HashMap<&'static str, &'static str>;
/// # type Response = ();
/// use mews::{WebSocketContext, Connection, Message};
/// 
/// async fn handle_websocket(
///     headers: Headers/* of upgrade request */,
///     tcp: TcpStream
/// ) -> Response {
///     let ctx = WebSocketContext::new(
///         &headers["Sec-WebSocket-Key"]
///     );
/// 
///     let (sign, ws) = ctx.on_upgrade(
///         |mut conn: Connection<TcpStream>| async move {
///             while let Ok(Some(Message::Text(text))) = conn.recv().await {
///                 conn.send(text).await
///                     .expect("failed to send message");
///                 sleep(Duration::from_secs(1)).await;
///             }
///         }
///     );
/// 
///     spawn(ws.manage(tcp));
/// 
///     /* return `Switching Protocol` response with `sign`... */
/// }
/// ```
pub struct WebSocketContext<'ctx> {
    sec_websocket_key: &'ctx str,
    config:            Config,
}
impl<'ctx> WebSocketContext<'ctx> {
    /// create `WebSocketContext` with `Sec-WebSocket-Key` request header value.
    pub fn new(sec_websocket_key: &'ctx str) -> Self {
        Self { sec_websocket_key, config: Config::default() }
    }

    pub fn with(mut self, config: Config) -> Self {
        self.config = config;
        self
    }

    /// create `Sec-WebSocket-Accept` value and a `WebSocket` with the handler.
    /// 
    /// ## handler
    /// 
    /// any 'static `FnOnce(Connection<C>) -> {impl Future<Output = ()> + Send} + Send + Sync`
    pub fn on_upgrade<C: UnderlyingConnection, H, F>(self, handler: H) -> (String, WebSocket<C>)
    where
        H: FnOnce(Connection<C>) -> F + Send + Sync + 'static,
        F: std::future::Future<Output = ()> + Send + 'static
    {
        (
            sign(self.sec_websocket_key),
            WebSocket {
                config:  self.config,
                handler: Box::new(|c| Box::pin(handler(c)))
            }
        )
    }
}
const _: () = {
    impl PartialEq for WebSocketContext<'_> {
        fn eq(&self, other: &Self) -> bool {
            self.sec_websocket_key == other.sec_websocket_key &&
            self.config == other.config
        }
    }

    impl std::fmt::Debug for WebSocketContext<'_> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("WebSocketContext")
                .field("Sec-WebSocket-Context", &self.sec_websocket_key)
                .field("config", &self.config)
                .finish()
        }
    }
};

/// WebSocket on underlying connection `C` (default: `TcpStream` of
/// selected async runtime)
/// 
/// **note** : `WebSocket` does nothing unless `.manage()` or `.manage_with_timeout()` is called.
/// 
/// *example.rs*
/// ```
/// # use tokio::time::{sleep, Duration};
/// # use tokio::{spawn, net::TcpStream};
/// # type Response = ();
/// use mews::{WebSocketContext, Connection, Message};
/// 
/// async fn handle_websocket(
///     ctx: WebSocketContext<'_>/* from upgrade request */,
///     tcp: TcpStream
/// ) -> Response {
///     let (sign, ws) = ctx.on_upgrade(
///         |mut conn: Connection<TcpStream>| async move {
///             while let Ok(Some(Message::Text(text))) = conn.recv().await {
///                 conn.send(text).await
///                     .expect("failed to send message");
///                 sleep(Duration::from_secs(1)).await;
///             }
///         }
///     );
/// 
///     spawn(ws.manage(tcp));
/// 
///     /* return `Switching Protocol` response with `sign`... */
/// }
/// ```
#[must_use = "`WebSocket` does nothing unless `.manage()` or `.manage_with_timeout()` is called"]
pub struct WebSocket<C: UnderlyingConnection> {
    config:  Config,
    handler: Handler<C>,
}
impl<C: UnderlyingConnection> WebSocket<C> {
    /// manage a WebSocket session on the connection.
    pub async fn manage(self, conn: C) {
        let (conn, closer) = Connection::new(conn, self.config);
        (self.handler)(conn).await;
        closer.send_close_if_not_closed().await;
    }

    /// manage a WebSocket session on the connection with a timeout future (like `sleep()`).
    /// 
    /// returns `true` if session has been aborted by the timeout.
    pub async fn manage_with_timeout(self, timeout: impl std::future::Future, conn: C) -> bool {
        let (conn, closer) = Connection::new(conn, self.config);

        if with_timeout(timeout, (self.handler)(conn)).await.is_none() {
            closer.send_close_if_not_closed_with(CloseFrame {
                code:   CloseCode::Library(4000),
                reason: Some("timeout".into())
            }).await;
            true
        } else {
            closer.send_close_if_not_closed_with(CloseFrame {
                code:   CloseCode::Normal,
                reason: None
            }).await;
            false
        }
    }
}
const _: () = {
    impl<C: UnderlyingConnection> PartialEq for WebSocket<C>
    where
        dyn FnOnce(Connection<C>) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'static>> + Send + Sync
        : PartialEq
    {
        fn eq(&self, other: &Self) -> bool {
            self.config == other.config &&
            &self.handler == &other.handler
        }
    }

    impl<C: UnderlyingConnection> std::fmt::Debug for WebSocket<C> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("WebSocket")
                .field("config", &self.config)
                .finish_non_exhaustive()
        }
    }
};

#[inline]
fn sign(sec_websocket_key: &str) -> String {
    use ::sha1::{Sha1, Digest};
    use ::base64::engine::{Engine, general_purpose::STANDARD};

    let mut sha1 = <Sha1 as Digest>::new();
    sha1.update(sec_websocket_key.as_bytes());
    sha1.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");

    Engine::encode(&STANDARD, sha1.finalize())
}

#[cfg(test)]
#[test] fn test_sign() {
    /* example of https://developer.mozilla.org/en-US/docs/Web/API/WebSockets_API/Writing_WebSocket_servers#server_handshake_response */
    assert_eq!(sign("dGhlIHNhbXBsZSBub25jZQ=="), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
}

#[inline]
fn with_timeout<T>(
    timer: impl std::future::Future,
    task: impl std::future::Future<Output = T>
) -> impl std::future::Future<Output = Option<T>> {
    return Timeout { timer, task };

    struct Timeout<Timer, Task> {
        timer: Timer,
        task: Task,
    }
    impl<Timer, Task, T> std::future::Future for Timeout<Timer, Task>
    where
        Timer: std::future::Future,
        Task: std::future::Future<Output = T>,
    {
        type Output = Option<T>;

        #[inline]
        fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
            use std::{task::Poll, pin::Pin};
            
            let Timeout { timer, task } = unsafe {self.get_unchecked_mut()};
            match unsafe {Pin::new_unchecked(task)}.poll(cx) {
                Poll::Ready(t) => Poll::Ready(Some(t)),
                Poll::Pending  => unsafe {Pin::new_unchecked(timer)}.poll(cx).map(|_| None)
            }
        }
    }
}
