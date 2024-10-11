use crate::{Config, Message};
use crate::runtime::{Read, Write, RwLock};
use std::{sync::Arc, io::Error};

/*
    Why 'static lifetime?
    1. The underlying connection is in `Arc`
    2. The closer returned from `Connection::new` is expected to be alive until WebSocket session completes
    3. This split is expected to be called before user's handler is called
*/
#[cfg(feature="tokio")]
pub trait UnderlyingConnection: Read + Write + Unpin + split::Splitable<'static> + 'static {}
#[cfg(feature="tokio")]
impl<T: Read + Write + Unpin + split::Splitable<'static> + 'static> UnderlyingConnection for T {}

#[cfg(not(feature="tokio"))]
pub trait UnderlyingConnection: Read + Write + Unpin + split::Splitable + 'static {}
#[cfg(not(feature="tokio"))]
impl<T: Read + Write + Unpin + split::Splitable + 'static> UnderlyingConnection for T {}

pub struct Connection<C: UnderlyingConnection> {
    /* FIXME: more sound structure */
    #[cfg(feature="tokio")]
    conn: Arc<std::cell::UnsafeCell<C>>,
    #[cfg(not(feature="tokio")/* TcpStream: Clone */)]
    conn: C,
    __closed__: Arc<RwLock<bool>>,
    config:     Config,
    n_buffered: usize,
}

unsafe impl<C: UnderlyingConnection> Send for Connection<C> {}
unsafe impl<C: UnderlyingConnection> Sync for Connection<C> {}

const _: () = {
    use crate::{CloseCode, CloseFrame};

    /// # WebSocket Connection Closer
    /// 
    /// Created together with `Connection` by `Connection::new`, and used to
    /// ensure sending close message to client before shutdown.
    /// 
    /// This is a workaround for that we can't perform async `Drop`
    /// in stable way. `Closer` is tend to be used like `Drop` process
    /// of the corresponded `Connection`.
    pub struct Closer<C: UnderlyingConnection>(Connection<C>);
    impl<C: UnderlyingConnection> Closer<C> {
        pub async fn send_close_if_not_closed(self) -> Result<(), Error> {
            self.send_close_if_not_closed_with(CloseFrame {
                code:   CloseCode::Normal,
                reason: None
            }).await
        }

        pub async fn send_close_if_not_closed_with(mut self, frame: CloseFrame) -> Result<(), Error> {
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
                self.0.send(Message::Close(Some(frame))).await?
            }

            Ok(())
        }
    }

    impl<C: UnderlyingConnection> Connection<C> {
        /// create 2 WebSocket connections for
        /// 
        /// 1. used to handle WebSocket session
        /// 2. used to ensure to send a close message (`Cloder`)
        /// 
        /// *example.rs*
        /// ```
        /// # use mews::{Connection, Config, Handler, Message, CloseCode, CloseFrame};
        /// #
        /// async fn upgrade_websocket(
        ///     connection: tokio::net::TcpStream,
        ///     config: Config,
        ///     handler: Handler,
        /// ) {
        ///     let (conn, closer) = Connection::new(connection, config);
        /// 
        ///     // 1. handle WebSocket session
        ///     handler(conn).await;
        /// 
        ///     // 2. send a close message if not already closed
        ///     closer.send_close_if_not_closed().await
        ///         .expect("failed to send a close frame");
        /// 
        ///     println!("WebSocket session finished")
        /// }
        /// ```
        pub fn new(conn: C, config: Config) -> (Self, Closer<C>) {
            #[cfg(feature="tokio")]
            let conn = Arc::new(std::cell::UnsafeCell::new(conn));

            let __closed__ = Arc::new(RwLock::new(false));

            (
                Self { conn: conn.clone(), __closed__: __closed__.clone(), config: config.clone(), n_buffered: 0 },
                Closer(Connection { conn, __closed__, config, n_buffered: 0 })
            )
        }

        pub async fn is_closed(&self) -> bool {
            *self.__closed__.read().await
        }

        pub(crate) async fn close(&mut self) {
            *self.__closed__.write().await = true
        }
    }

    impl<C: UnderlyingConnection + std::fmt::Debug> std::fmt::Debug for Connection<C> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("WebSocket Connection")
                .field("underlying", {
                    #[cfg(feature="tokio")] {
                        &unsafe {&*self.conn.get()}
                    }
                    #[cfg(not(feature="tokio"))] {
                        &self.conn
                    }
                })
                .field("config", &self.config)
                .field("n_buffered", &self.n_buffered)
                .finish()
        }
    }
};

/*===========================================================================*/
#[inline]
pub(super) async fn send(
    message:    Message,
    conn:       &mut (impl Write + Unpin),
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
    conn:       &mut (impl Write + Unpin),
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
    conn:       &mut (impl Write + Unpin),
    n_buffered: &mut usize,
) -> Result<(), Error> {
    conn.flush().await
        .map(|_| *n_buffered = 0)
}
/*===========================================================================*/

macro_rules! underlying {
    ($this:expr) => {async {
        let _: &mut _ = $this;
        let conn = (!*$this.__closed__.read().await).then(|| {
            #[cfg(feature="tokio")] {
                // SAFETY: `$this` has unique access to `$this.conn` due to the
                // mutable = exclusive reference
                unsafe {&mut *$this.conn.get()}
            }
            #[cfg(not(feature="tokio"))] {
                &mut $this.conn
            }
        });
        underlying!(@@checked conn)
    }};
    (@split $__closed__:ident, $conn:ident) => {async {
        let _: &mut _ = $conn;
        let conn = if (!*$__closed__.read().await) {Some($conn)} else {None};
        underlying!(@@checked conn)
    }};
    (@@checked $this:expr) => {{
        let _: Option<&mut _> = $this;
        $this.ok_or_else(|| {
            #[cfg(debug_assertions)] eprintln! {"\n\
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
            "}
            ::std::io::Error::new(::std::io::ErrorKind::ConnectionReset, "WebSocket connection is already closed")
        })
    }};
}

impl<C: UnderlyingConnection> Connection<C> {
    /// Await a message from the client and recieve it.
    /// 
    /// **note** : This automatically consumes a `Ping` message and responds with
    /// a corresponded `Pong` message, and then returns `Ok(None)`.
    #[inline]
    pub async fn recv(&mut self) -> Result<Option<Message>, Error> {
        #[cfg(feature="tokio")]
        let conn = underlying!(self).await?;

        #[cfg(not(feature="tokio"))]
        let Self { conn, __closed__, .. } = self;
        #[cfg(not(feature="tokio"))]
        let conn = underlying!(@split __closed__, conn).await?;

        match Message::read_from(conn, &self.config).await? {
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
    pub async fn send(&mut self, message: Message) -> Result<(), Error> {
        #[cfg(feature="tokio")]
        let conn = underlying!(self).await?;

        #[cfg(not(feature="tokio"))]
        let Self { conn, __closed__, .. } = self;
        #[cfg(not(feature="tokio"))]
        let conn = underlying!(@split __closed__, conn).await?;

        let closing = matches!(message, Message::Close(_));
        send(message, conn, &self.config, &mut self.n_buffered).await?;
        if closing {self.close().await}
        Ok(())
    }

    /// Write a message to the connection. Buffering behavior is customizable
    /// via `Config` of `WebSocketContext::connect_with`.
    /// 
    /// **note** : When sending a `Close` message, this automatically close the
    /// connection, then the connection is not available anymore.
    pub async fn write(&mut self, message: Message) -> Result<usize, Error> {
        #[cfg(feature="tokio")]
        let conn = underlying!(self).await?;

        #[cfg(not(feature="tokio"))]
        let Self { conn, __closed__, .. } = self;
        #[cfg(not(feature="tokio"))]
        let conn = underlying!(@split __closed__, conn).await?;

        let closing = matches!(message, Message::Close(_));
        let n = write(message, conn, &self.config, &mut self.n_buffered).await?;
        if closing {self.close().await}
        Ok(n)
    }

    /// Flush the connection explicitly.
    pub async fn flush(&mut self) -> Result<(), Error> {
        #[cfg(feature="tokio")]
        let conn = underlying!(self).await?;

        #[cfg(not(feature="tokio"))]
        let Self { conn, __closed__, .. } = self;
        #[cfg(not(feature="tokio"))]
        let conn = underlying!(@split __closed__, conn).await?;

        flush(conn, &mut self.n_buffered).await
    }
}

pub mod split {
    use super::*;
    
    #[cfg(feature="tokio")]
    pub trait Splitable<'split>: Read + Write + Unpin + Sized {
        type ReadHalf: Read + Unpin;
        type WriteHalf: Write + Unpin;
        fn split(&'split mut self) -> (Self::ReadHalf, Self::WriteHalf);
    }
    #[cfg(feature="tokio")]
    impl<'split> Splitable<'split> for tokio::net::TcpStream {
        type ReadHalf = tokio::net::tcp::ReadHalf<'split>;
        type WriteHalf = tokio::net::tcp::WriteHalf<'split>;
        fn split(&'split mut self) -> (Self::ReadHalf, Self::WriteHalf) {
            <tokio::net::TcpStream>::split(self)
        }
    }

    #[cfg(not(feature="tokio"))]
    pub trait Splitable: Read + Write + Unpin + Sized {
        type ReadHalf: Read + Unpin;
        type WriteHalf: Write + Unpin;
        fn split(self) -> (Self::ReadHalf, Self::WriteHalf);
    }
    #[cfg(not(feature="tokio"))]
    const _: () = {
        #[cfg(not(feature="glommio"))]
        impl<C: Read + Write + Unpin + Sized + Clone> Splitable for C {
            type ReadHalf = Self;
            type WriteHalf = Self;
            fn split(self) -> (Self::ReadHalf, Self::WriteHalf) {
                (self.clone(), self)
            }
        }
        #[cfg(feature="glommio")]
        impl<C: Read + Write + Unpin + Sized> Splitable for C {
            type ReadHalf = ::futures_util::io::ReadHalf<Self>;
            type WriteHalf = ::futures_util::io::WriteHalf<Self>;
            fn split(self) -> (Self::ReadHalf, Self::WriteHalf) {
                Read::split(self)
            }
        }
    };

    pub struct ReadHalf<C: Read + Unpin> {
        __closed__: Arc<RwLock<bool>>,
        conn:   C,
        config: Config,
    }
    impl<C: Read + Unpin> ReadHalf<C> {
        /// Await a message from the client and recieve it.
        /// 
        /// **note** : This doesn't automatically handle `Ping` message
        /// (in contrast to `Connection::recv`).
        #[inline]
        pub async fn recv(&mut self) -> Result<Option<Message>, Error> {
            let Self { __closed__, conn, config } = self;
            let conn = underlying!(@split __closed__, conn).await?;
            Message::read_from(conn, config).await
        }
    }

    pub struct WriteHalf<C: Write + Unpin> {
        __closed__: Arc<RwLock<bool>>,
        conn:       C,
        config:     Config,
        n_buffered: usize,
    }
    impl<C: Write + Unpin> WriteHalf<C> {
        /// Send a message to the client.
        /// 
        /// **note** : When sending a `Close` message, this automatically close the
        /// connection, then the connection is not available anymore.
        #[inline]
        pub async fn send(&mut self, message: Message) -> Result<(), Error> {
            let Self { __closed__, conn, config, n_buffered } = self;
            let conn = underlying!(@split __closed__, conn).await?;
            let closing = matches!(message, Message::Close(_));
            send(message, conn, config, n_buffered).await?;
            if closing {*__closed__.write().await = true}
            Ok(())
        }

        /// Write a message to the connection. Buffering behavior is customizable
        /// via `Config` of `WebSocketContext::connect_with`.
        /// 
        /// **note** : When sending a `Close` message, this automatically close the
        /// connection, then the connection is not available anymore.
        pub async fn write(&mut self, message: Message) -> Result<usize, Error> {
            let Self { __closed__, conn, config, n_buffered } = self;
            let conn = underlying!(@split __closed__, conn).await?;
            let closing = matches!(message, Message::Close(_));
            let n = write(message, conn, config, n_buffered).await?;
            if closing {*__closed__.write().await = true}
            Ok(n)
        }

        /// Flush the connection explicitly.
        pub async fn flush(&mut self) -> Result<(), Error> {
            let Self { __closed__, conn, n_buffered, config:_ } = self;
            let conn = underlying!(@split __closed__, conn).await?;
            flush(conn, n_buffered).await
        }
    }

    impl<C: UnderlyingConnection> Connection<C> {
        /// SAFETY: MUST be called before user's handler
        pub(crate) unsafe fn split(self) -> (ReadHalf<C::ReadHalf>, WriteHalf<C::WriteHalf>) {
            #[cfg(feature="tokio")]
            let conn = &mut *self.conn.get();

            #[cfg(not(feature="tokio"))]
            let conn = self.conn;

            let (r, w) = conn.split();
            (
                ReadHalf  {
                    __closed__: self.__closed__.clone(),
                    conn: r,
                    config: self.config.clone()
                },
                WriteHalf {
                    __closed__: self.__closed__,
                    conn: w,
                    config: self.config,
                    n_buffered: self.n_buffered
                },
            )
        }
    }
}
