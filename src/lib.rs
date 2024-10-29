//! <div align="center">
//!     <h1>MEWS</h1>
//!     Minimal and Efficient, Multiple-Environment WebSocket implementation for async Rust
//! </div>
//! 
//! <br>
//! 
//! <div align="right">
//!     <a href="https://github.com/ohkami-rs/mews/blob/main/LICENSE"><img alt="License" src="https://img.shields.io/crates/l/mews.svg" /></a>
//!     <a href="https://github.com/ohkami-rs/mews/actions"><img alt="CI status" src="https://github.com/ohkami-rs/mews/actions/workflows/CI.yml/badge.svg"/></a>
//!     <a href="https://crates.io/crates/mews"><img alt="crates.io" src="https://img.shields.io/crates/v/mews" /></a>
//! </div>
//! 
//! ## Note
//! 
//! MEWS is NOT WebSocket server, just protocol implementation. So :
//! 
//! * Tend to be used by web frameworks internally, not by end-developers.
//! 
//! * Doesn't builtins `wss://` support.
//! 
//! ## Features
//! 
//! * Minimal and Efficient : minimal codebase to provide efficient, memory-safe WebSocket handling.
//! 
//! * Multiple Environment : `tokio`, `async-std`, `smol`, `glommio` are supported as async runtime ( by feature flags of the names ).

#[cfg(not(any(feature="tokio", feature="async-std", feature="smol", feature="glommio")))]
compile_error! {"One of runtime feature flags ( tokio, async-std, smol, glommio ) must be activated"}

#[cfg(any(
    all(feature="tokio", any(feature="async-std",feature="smol",feature="glommio")),
    all(feature="async-std", any(feature="smol",feature="glommio",feature="tokio")),
    all(feature="smol", any(feature="glommio",feature="tokio",feature="async-std")),
    all(feature="glommio", any(feature="tokio", feature="async-std",feature="smol",)),
))]
compile_error! {"More than one runtime feature flags can't be activated"}

mod runtime {
    #[cfg(feature="tokio")]
    pub use {
        tokio::net::TcpStream,
        tokio::io::AsyncReadExt as Read,
        tokio::io::AsyncWriteExt as Write,
        tokio::sync::RwLock,
        tokio::time::sleep
    };

    #[cfg(feature="async-std")]
    pub use {
        async_std::net::TcpStream,
        async_std::io::ReadExt as Read,
        async_std::io::WriteExt as Write,
        async_std::sync::RwLock,
        async_std::task::sleep
    };

    #[cfg(feature="smol")]
    pub use {
        smol::net::TcpStream,
        smol::io::AsyncReadExt as Read,
        smol::io::AsyncWriteExt as Write,
        smol::lock::RwLock,
    };
    #[cfg(feature="smol")]
    pub async fn sleep(duration: std::time::Duration) {
        smol::Timer::after(duration).await;
    }

    #[cfg(feature="glommio")]
    pub use {
        glommio::net::TcpStream,
        futures_util::AsyncReadExt as Read,
        futures_util::AsyncWriteExt as Write,
        glommio::sync::RwLock,
        glommio::timer::sleep
    };
}

pub mod connection;
pub mod handler;
pub mod frame;
pub mod message;

pub use connection::Connection;
pub use connection::split::{self, ReadHalf, WriteHalf};
pub use handler::Handler;
pub use frame::CloseCode;
pub use message::{Message, CloseFrame};

///////////////////////////////////////////////////////////////////////////

pub(crate) use connection::UnderlyingConnection;

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
///         |mut conn: Connection| async move {
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
    /// Any `FnOnce + Send + Sync` returning `Send + Future`
    /// with following args and `Output`:
    /// 
    /// * `(Connection) -> () | std::io::Result<()>`
    /// * `(ReadHalf, WriteHalf) -> () | std::io::Result<()>`
    pub fn on_upgrade<C: UnderlyingConnection, T>(self, handler: impl handler::IntoHandler<C, T>) -> (String, WebSocket<C>) {
        (
            sign(self.sec_websocket_key),
            WebSocket {
                config:  self.config,
                handler: handler.into_handler(),
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
///         |mut conn: Connection| async move {
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
pub struct WebSocket<C: UnderlyingConnection = runtime::TcpStream> {
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

    /// manage a WebSocket session on the connection with timeout.
    /// 
    /// returns `true` if session has been aborted by the timeout.
    pub async fn manage_with_timeout(self, timeout: std::time::Duration, conn: C) -> bool {
        let (conn, closer) = Connection::new(conn, self.config);

        let is_timeouted = crate::timeout(timeout,
            (self.handler)(conn)
        ).await.is_none();

        if is_timeouted {
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
        dyn FnOnce(connection::Connection<C>) -> std::pin::Pin<Box<(dyn std::future::Future<Output = ()> + Send + 'static)>> + Send + Sync
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
fn timeout<T>(
    duration: std::time::Duration,
    task: impl std::future::Future<Output = T>
) -> impl std::future::Future<Output = Option<T>> {
    use std::{task::Poll, pin::Pin};

    struct Timeout<Sleep, Proc> { sleep: Sleep, task: Proc }

    #[cfg(feature="glommio")]
    // SAFETY: task and sleep are performed on same thread
    unsafe impl<Sleep, Proc> Send for Timeout<Sleep, Proc> {}

    impl<Sleep, Proc, T> std::future::Future for Timeout<Sleep, Proc>
    where
        Sleep: std::future::Future<Output = ()>,
        Proc:  std::future::Future<Output = T>,
    {
        type Output = Option<T>;

        #[inline]
        fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
            let Timeout { sleep, task } = unsafe {self.get_unchecked_mut()};
            match unsafe {Pin::new_unchecked(task)}.poll(cx) {
                Poll::Ready(t) => Poll::Ready(Some(t)),
                Poll::Pending  => unsafe {Pin::new_unchecked(sleep)}.poll(cx).map(|_| None)
            }
        }
    }

    Timeout { task, sleep: crate::runtime::sleep(duration) }
}
