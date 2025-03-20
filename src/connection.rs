use crate::{Config, Message};
use crate::runtime::{AsyncRead, AsyncWrite, RwLock};
use std::{sync::Arc, io::Error};

pub trait UnderlyingConnection: AsyncRead + AsyncWrite + Unpin + 'static {}
impl<T: AsyncRead + AsyncWrite + Unpin + 'static> UnderlyingConnection for T {}

pub struct Connection<C: UnderlyingConnection = crate::runtime::TcpStream> {
    __closed__: Arc<RwLock<bool>>,

    conn: Arc<std::cell::UnsafeCell<C>>,

    config:     Config,
    n_buffered: usize,
}

/*============================================================*/
/* utils                                                      */
/*============================================================*/
    #[inline(always)]
    async fn read_closed(__closed__: &RwLock<bool>) -> bool {
        #[cfg(feature="rt_glommio")]
        match __closed__.read().await {
            Ok(read) => *read,
            Err(_/* closed */) => true
        }
        #[cfg(not(feature="rt_glommio"))]
        *__closed__.read().await
    }
    #[inline(always)]
    async fn set_closed(__closed__: &RwLock<bool>) {
        #[cfg(feature="rt_glommio")] {
            if let Ok(mut write) = __closed__.write().await {
                *write = true
            }
        }
        #[cfg(not(feature="rt_glommio"))] {
            *__closed__.write().await = true
        }
    }

    const ALREADY_CLOSED_MESSAGE: &str = "\n\
        |--------------------------------------------\n\
        | WebSocket connection is already closed!   |\n\
        |                                           |\n\
        | Maybe you spawned tasks using connection  |\n\
        | and NOT join/await the tasks?             |\n\
        |                                           |\n\
        | This is NOT supported because it may      |\n\
        | cause resource leak due to something like |\n\
        | an infinite loop or a dead lock in the    |\n\
        | WebSocket handler.                        |\n\
        | If you're doing it, please join/await the |\n\
        | tasks in the handler!                     |\n\
        --------------------------------------------|\n\
    ";

    macro_rules! underlying {
        ($this:expr) => {async {
            let _: &mut Connection<_> = $this;
            let conn = (!read_closed(&$this.__closed__).await).then(|| {
                // SAFETY: `$this` has unique access to `$this.conn` due to the
                // mutable = exclusive reference
                // 
                // (this is based on the precondition that: `$this` and the closer are NOT used at the same time)
                unsafe {&mut *$this.conn.get()}
            });
            underlying!(@@checked conn)
        }};
        ($__closed__:ident, $conn:ident) => {async {
            let _: &mut _ = $conn;
            let conn = if (!read_closed(&$__closed__).await) {Some($conn)} else {None};
            underlying!(@@checked conn)
        }};
        (@@checked $this:expr) => {{
            let _: Option<&mut _> = $this;
            $this.ok_or_else(|| {
                #[cfg(debug_assertions)] eprintln! {"{ALREADY_CLOSED_MESSAGE}"}
                ::std::io::Error::new(::std::io::ErrorKind::ConnectionReset, "WebSocket connection is already closed")
            })
        }};
    }
    #[inline(always)]
    async fn to_checked_parts<C: UnderlyingConnection>(connection: &mut Connection<C>) -> Result<(&mut C, &Config, &mut usize), Error> {
        let conn = underlying!(connection).await?;
        return Ok((conn, &connection.config, &mut connection.n_buffered))
    }

    #[inline]
    pub(super) async fn send(
        message:    Message,
        conn:       &mut (impl AsyncWrite + Unpin),
        config:     &Config,
        n_buffered: &mut usize,
    ) -> Result<(), Error> {
        message.write(conn, config).await?;
        flush(conn, n_buffered).await?;
        Ok(())
    }
    #[inline]
    pub(super) async fn write(
        message:    Message,
        conn:       &mut (impl AsyncWrite + Unpin),
        config:     &Config,
        n_buffered: &mut usize,
    ) -> Result<usize, Error> {
        let n = message.write(conn, config).await?;
        *n_buffered += n;
        if *n_buffered > config.write_buffer_size {
            if *n_buffered > config.max_write_buffer_size {
                panic!("Buffered messages is larger than `max_write_buffer_size`");
            } else {
                flush(conn, n_buffered).await?
            }
        }
        Ok(n)
    }
    #[inline]
    pub(super) async fn flush(
        conn:       &mut (impl AsyncWrite + Unpin),
        n_buffered: &mut usize,
    ) -> Result<(), Error> {
        conn.flush().await
            .map(|_| *n_buffered = 0)
    }
/*============================================================*/
/* end utils                                                  */
/*============================================================*/

const _: (/* trait impls */) = {
    unsafe impl<C: UnderlyingConnection> Send for Connection<C> {}
    unsafe impl<C: UnderlyingConnection> Sync for Connection<C> {}

    impl<C: UnderlyingConnection + std::fmt::Debug> std::fmt::Debug for Connection<C> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("WebSocket Connection")
                .field("underlying", &unsafe {&*self.conn.get()})
                .field("config", &self.config)
                .field("n_buffered", &self.n_buffered)
                .finish()
        }
    }
};

/// # WebSocket Connection Closer
/// 
/// Created together with `Connection` by `Connection::new`, and used to
/// ensure sending close message to client before shutdown.
/// 
/// This is a workaround for that we can't perform async `Drop`
/// in stable way. `Closer` is tend to be used like `Drop` process
/// of the corresponded `Connection`.
pub struct Closer<C: UnderlyingConnection>(Connection<C>);

use crate::{CloseCode, CloseFrame};
impl<C: UnderlyingConnection> Closer<C> {
    /// if the connection is not closed yet, send a close frame with
    /// `CloseCode::Normal`. see [`send_close_if_not_closed_with`](Closer::send_close_if_not_closed_with)
    /// to do with custom frame.
    pub async fn send_close_if_not_closed(self) {
        self.send_close_if_not_closed_with(CloseFrame {
            code:   CloseCode::Normal,
            reason: None
        }).await
    }

    /// if the connection is not closed yet, send the close frame.
    pub async fn send_close_if_not_closed_with(mut self, frame: CloseFrame) {
        #[cfg(debug_assertions)] {
            if Arc::strong_count(&self.0.__closed__) != 1 {
                eprintln!("\n\
                    Unexpected state of WebSocket closer found!\n\
                    \n\
                    First use `Connection` in a `Handler`,\n\
                    and next use `Closer` to ensure to \n\
                    send a close message to client.\n\
                ")
            }
        }

        if !self.0.is_closed().await {
            if let Err(e) = self.0.send(Message::Close(Some(frame))).await {
                eprintln!("failed to send a close message: {e}")
            }
        }
    }
}

impl<C: UnderlyingConnection> Connection<C> {
    /// create 2 WebSocket connections for
    /// 
    /// 1. used to handle WebSocket session
    /// 2. used to ensure to send a close message (`Closer`)
    /// 
    /// *example.rs*
    /// ```
    /// # use mews::{Connection, Config, Handler, Message, CloseCode, CloseFrame};
    /// #
    /// async fn upgrade_websocket(
    ///     connection: tokio::net::TcpStream,
    ///     config: Config,
    ///     handler: Handler<tokio::net::TcpStream>,
    /// ) {
    ///     let (conn, closer) = Connection::new(connection, config);
    /// 
    ///     // 1. handle WebSocket session
    ///     handler(conn).await;
    /// 
    ///     // 2. send a close message if not already closed
    ///     closer.send_close_if_not_closed().await;
    /// 
    ///     println!("WebSocket session finished")
    /// }
    /// ```
    pub fn new(conn: C, config: Config) -> (Self, Closer<C>) {
        let conn = Arc::new(std::cell::UnsafeCell::new(conn));
        let __closed__ = Arc::new(RwLock::new(false));
        (
            Self { conn: conn.clone(), __closed__: __closed__.clone(), config: config.clone(), n_buffered: 0 },
            Closer(Connection { conn, __closed__, config, n_buffered: 0 })
        )
    }

    pub async fn is_closed(&self) -> bool {
        read_closed(&self.__closed__).await
    }

    pub(crate) async fn close(&mut self) {
        set_closed(&self.__closed__).await
    }
}

impl<C: UnderlyingConnection> Connection<C> {
    /// Await a message from the client and recieve it.
    /// 
    /// **note** : This automatically consumes a `Ping` message and responds with
    /// a corresponded `Pong` message, and then returns `Ok(None)`.
    #[inline]
    pub async fn recv(&mut self) -> Result<Option<Message>, Error> {
        let (conn, config, _) = to_checked_parts(self).await?;

        match Message::read_from(conn, config).await? {
            Some(Message::Ping(payload)) => {
                self.send(Message::Pong(payload.clone())).await?;
                Ok(None)
            }
            other => Ok(other)
        }
    }

    /// Send a message to the client.
    /// 
    /// **note** : When sending a `Close` message, this automatically close the
    /// connection, then the connection is not available anymore.
    #[inline]
    pub async fn send(&mut self, message: impl Into<Message>) -> Result<(), Error> {
        let message = message.into();

        let (conn, config, n_buffered) = to_checked_parts(self).await?;

        let closing = matches!(message, Message::Close(_));
        send(message, conn, config, n_buffered).await?;
        if closing {self.close().await}

        Ok(())
    }

    /// Write a message to the connection. Buffering behavior is customizable
    /// via [`WebSocketContext::with(Config)`](crate::WebSocketContext::with).
    /// 
    /// **note** : When sending a `Close` message, this automatically close the
    /// connection, then the connection is not available anymore.
    pub async fn write(&mut self, message: impl Into<Message>) -> Result<usize, Error> {
        let message = message.into();

        let (conn, config, n_buffered) = to_checked_parts(self).await?;

        let closing = matches!(message, Message::Close(_));
        let n = write(message, conn, config, n_buffered).await?;
        if closing {self.close().await}

        Ok(n)
    }

    /// Flush the connection explicitly.
    pub async fn flush(&mut self) -> Result<(), Error> {
        let (conn, _, n_buffered) = to_checked_parts(self).await?;
        flush(conn, n_buffered).await
    }
}

pub mod split {
    use super::*;
    
    pub trait Splitable<'split>: AsyncRead + AsyncWrite + Unpin + Sized {
        type ReadHalf: AsyncRead + Unpin;
        type WriteHalf: AsyncWrite + Unpin;
        fn split(&'split mut self) -> (Self::ReadHalf, Self::WriteHalf);
    }

    impl<C: UnderlyingConnection> Connection<C>
    where
        C: for<'s> Splitable<'s>,
    {
        /// ## Panics
        /// 
        /// This panics if the original `Connection` is already closed.
        pub fn split(self) -> (
            ReadHalf<<C as Splitable<'static>>::ReadHalf>,
            WriteHalf<<C as Splitable<'static>>::WriteHalf>,
        ) {
            if !(*self.__closed__.try_read().expect(ALREADY_CLOSED_MESSAGE) == false) {
                panic!("{ALREADY_CLOSED_MESSAGE}")
            }

            let conn = unsafe {&mut *self.conn.get()};

            let (r, w) = conn.split();
            let __closed__ = Arc::new(RwLock::new(false));
            (
                ReadHalf  {
                    __closed__: __closed__.clone(),
                    conn: r,
                    config: self.config.clone()
                },
                WriteHalf {
                    __closed__,
                    conn: w,
                    config: self.config,
                    n_buffered: self.n_buffered
                },
            )
        }
    }

    #[cfg(feature="__splitref__")]
    const _: () = {
        #[cfg(feature="rt_tokio")]
        impl<'split> Splitable<'split> for tokio::net::TcpStream {
            type ReadHalf  = tokio::net::tcp::ReadHalf <'split>;
            type WriteHalf = tokio::net::tcp::WriteHalf<'split>;
            fn split(&'split mut self) -> (Self::ReadHalf, Self::WriteHalf) {
                <tokio::net::TcpStream>::split(self)
            }
        }
        #[cfg(feature="rt_nio")]
        impl<'split> Splitable<'split> for nio::net::TcpStream {
            type ReadHalf  = nio::net::tcp::ReadHalf <'split>;
            type WriteHalf = nio::net::tcp::WriteHalf<'split>;
            fn split(&'split mut self) -> (Self::ReadHalf, Self::WriteHalf) {
                <nio::net::TcpStream>::split(self)
            }
        }
        #[cfg(feature="rt_glommio")]
        impl<'split, T: AsyncRead + AsyncWrite + Unpin + 'split> Splitable<'split> for T {
            type ReadHalf  = futures_util::io::ReadHalf <&'split mut T>;
            type WriteHalf = futures_util::io::WriteHalf<&'split mut T>;
            fn split(&'split mut self) -> (Self::ReadHalf, Self::WriteHalf) {
                AsyncRead::split(self)
            }
        }
    };
    #[cfg(feature="__clone__")]
    const _: () = {
        impl<'split, C: AsyncRead + AsyncWrite + Unpin + Sized + Clone + 'split> Splitable<'split> for C {
            type ReadHalf = Self;
            type WriteHalf = &'split mut Self;
            fn split(&'split mut self) -> (Self::ReadHalf, Self::WriteHalf) {
                (self.clone(), self)
            }
        }
    };

    pub struct ReadHalf<C: AsyncRead + Unpin = <crate::runtime::TcpStream as Splitable<'static>>::ReadHalf> {
        __closed__: Arc<RwLock<bool>>,
        conn:   C,
        config: Config,
    }
    impl<C: AsyncRead + Unpin> ReadHalf<C> {
        /// Await a message from the client and recieve it.
        /// 
        /// **note** : This doesn't automatically handle `Ping` message
        /// (in contrast to `Connection::recv`).
        #[inline]
        pub async fn recv(&mut self) -> Result<Option<Message>, Error> {
            let Self { __closed__, conn, config } = self;
            let conn = underlying!(__closed__, conn).await?;
            Message::read_from(conn, config).await
        }
    }

    pub struct WriteHalf<C: AsyncWrite + Unpin = <crate::runtime::TcpStream as Splitable<'static>>::WriteHalf> {
        __closed__: Arc<RwLock<bool>>,
        conn:       C,
        config:     Config,
        n_buffered: usize,
    }
    impl<C: AsyncWrite + Unpin> WriteHalf<C> {
        /// Send a message to the client.
        /// 
        /// **note** : When sending a `Close` message, this automatically close the
        /// connection, then the connection is not available anymore.
        #[inline]
        pub async fn send(&mut self, message: impl Into<Message>) -> Result<(), Error> {
            let message = message.into();

            let Self { __closed__, conn, config, n_buffered } = self;
            let conn = underlying!(__closed__, conn).await?;

            let closing = matches!(message, Message::Close(_));
            send(message, conn, config, n_buffered).await?;
            if closing {set_closed(__closed__).await}

            Ok(())
        }

        /// Write a message to the connection. Buffering behavior is customizable
        /// via [`WebSocketContext::with(Config)`](crate::WebSocketContext::with).
        /// 
        /// **note** : When sending a `Close` message, this automatically close the
        /// connection, then the connection is not available anymore.
        pub async fn write(&mut self, message: impl Into<Message>) -> Result<usize, Error> {
            let message = message.into();

            let Self { __closed__, conn, config, n_buffered } = self;
            let conn = underlying!(__closed__, conn).await?;

            let closing = matches!(message, Message::Close(_));
            let n = write(message, conn, config, n_buffered).await?;
            if closing {set_closed(__closed__).await}

            Ok(n)
        }

        /// Flush the connection explicitly.
        pub async fn flush(&mut self) -> Result<(), Error> {
            let Self { __closed__, conn, n_buffered, config:_ } = self;
            let conn = underlying!(__closed__, conn).await?;

            flush(conn, n_buffered).await
        }
    }
}
