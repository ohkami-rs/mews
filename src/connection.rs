use crate::{Config, Message};
use crate::runtime::{Read, Write};
use std::{sync::Arc, cell::UnsafeCell, io::Error};

pub trait UnderlyingConnection: Read + Write + Unpin + split::Splitable<'static> + 'static {}
impl<T: Read + Write + Unpin + split::Splitable<'static> + 'static> UnderlyingConnection for T {}

pub struct Connection<C: UnderlyingConnection> {
    conn:       Arc<UnsafeCell<Option<C>>>,
    config:     Config,
    n_buffered: usize,
}

unsafe impl<C: UnderlyingConnection> Send for Connection<C> {}
unsafe impl<C: UnderlyingConnection> Sync for Connection<C> {}

const _: () = {
    type Closer<C> = Connection<C>;

    impl<C: UnderlyingConnection> Connection<C> {
        /* this requires `Arc<{C with inner mutability}>` */

        /// create 2 WebSocket connections for
        /// 
        /// 1. handling WebSocket session
        /// 2. sending a close message
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
        ///     let (conn, mut closer) = Connection::new(connection, config);
        /// 
        ///     // 1. handle WebSocket session
        ///     handler(conn).await;
        /// 
        ///     // 2. send a close message if not already closed
        ///     if !closer.is_closed() {
        ///         closer.send(Message::Close(Some(CloseFrame {
        ///             code: CloseCode::Normal,
        ///             reason: None
        ///         }))).await.expect("failed to send a close frame");
        ///     }
        /// 
        ///     println!("WebSocket session finished")
        /// }
        /// ```
        pub fn new(conn: C, config: Config) -> (Self, Closer<C>) {
            let conn = Arc::new(UnsafeCell::new(Some(conn)));
            (
                Self { conn: conn.clone(), config: config.clone(), n_buffered: 0 },
                Closer { conn, config, n_buffered: 0 }
            )
        }

        pub fn is_closed(&self) -> bool {
            unsafe {&*self.conn.get()}.is_none()
        }    

        pub(crate) fn close(&mut self) {
            // SAFETY: `&mut self` has unique access to `self.conn`
            *unsafe {&mut *self.conn.get()} = None
        }
    }

    impl<C: UnderlyingConnection + std::fmt::Debug> std::fmt::Debug for Connection<C> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("WebSocket Connection")
                .field("underlying", unsafe {&*self.conn.get()})
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

/// SAFETY: `self` must have unique access to `self.conn`
macro_rules! underlying {
    ($this:expr) => {{
        let _: &mut _ = $this;
        // SAFETY: `&mut $this` has unique access to `$this.conn` due to the
        // mutable = exclusive reference
        unsafe {&mut *$this.conn.get()}.as_mut().ok_or_else(|| {
            #[cfg(debug_assertions)] eprintln! {"\n\
                |-----------------------------------------------------------------------\n\
                | WebSocket connection is already closed!                               |\n\
                |                                                                      |\n\
                | Maybe you spawned tasks using mews::Connection or split halves of it |\n\
                | and NOT join/await the tasks?                                        |\n\
                | This is NOT supported because it may cause resource leak             |\n\
                | due to something like an infinite loop or a dead lock in the         |\n\
                | websocket handler.                                                   |\n\
                | If you're doing it, please join/await the tasks in the handler!      |\n\
                -----------------------------------------------------------------------|\n\
            "}
            ::std::io::Error::new(::std::io::ErrorKind::ConnectionReset, "WebSocket connection is already closed")
        })
    }}
}

impl<C: UnderlyingConnection> Connection<C> {
    /// Await a message from the client and recieve it.
    /// 
    /// **note** : This automatically consumes a `Ping` message and responds with
    /// a corresponded `Pong` message, and then returns `Ok(None)`.
    #[inline]
    pub async fn recv(&mut self) -> Result<Option<Message>, Error> {
        let conn = underlying!(self)?;
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
        let conn = underlying!(self)?;
        let closing = matches!(message, Message::Close(_));
        send(message, conn, &self.config, &mut self.n_buffered).await?;
        if closing {self.close()}
        Ok(())
    }

    /// Write a message to the connection. Buffering behavior is customizable
    /// via `Config`.
    /// 
    /// **note** : When sending a `Close` message, this automatically close the
    /// connection, then the connection is not available anymore.
    pub async fn write(&mut self, message: Message) -> Result<usize, Error> {
        let conn = underlying!(self)?;
        let closing = matches!(message, Message::Close(_));
        let n = write(message, conn, &self.config, &mut self.n_buffered).await?;
        if closing {self.close()}
        Ok(n)
    }

    /// Flush the connection explicitly.
    pub async fn flush(&mut self) -> Result<(), Error> {
        let conn = underlying!(self)?;
        flush(conn, &mut self.n_buffered).await
    }
}

pub mod split {
    use super::*;
    
    pub trait Splitable<'split>: Read + Write + Unpin + Sized {
        type ReadHalf: Read + Unpin;
        type WriteHalf: Write + Unpin;
        fn split(&'split mut self) -> (Self::ReadHalf, Self::WriteHalf);
    }

    pub struct ReadHalf<C: Read + Unpin> {
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
            Message::read_from(&mut self.conn, &self.config).await
        }
    }

    pub struct WriteHalf<C: Write + Unpin> {
        conn:       C,
        config:     Config,
        n_buffered: usize,
    }
    impl<C: Write + Unpin> WriteHalf<C> {
        #[inline]
        pub async fn send(&mut self, message: Message) -> Result<(), Error> {
            send(message, &mut self.conn, &self.config, &mut self.n_buffered).await
        }

        pub async fn write(&mut self, message: Message) -> Result<usize, Error> {
            write(message, &mut self.conn, &self.config, &mut self.n_buffered).await
        }

        pub async fn flush(&mut self) -> Result<(), Error> {
            flush(&mut self.conn, &mut self.n_buffered).await
        }
    }

    /*
        Why 'static lifetime?

        1. The underlying connection is in `Arc`
        2. The closer returned from `Connection::new` is
           expected to be alive until WebSocket session completes
        3. This split is expected to be called before user's handler
           is called
    */
    impl<C: UnderlyingConnection> Connection<C> {
        pub(crate) fn split(mut self) -> Result<(ReadHalf<C::ReadHalf>, WriteHalf<C::WriteHalf>), Error> {
            let conn = underlying!(&mut self)?;
            let (r, w) = conn.split();
            Ok((
                ReadHalf  { conn: r, config: self.config.clone() },
                WriteHalf { conn: w, config: self.config, n_buffered: self.n_buffered },
            ))
        }
    }

    #[cfg(feature="tokio")]
    mod split_impl {
        use super::*;
        // use tokio::net::tcp::{ReadHalf as TcpReadHalf, WriteHalf as TcpWriteHalf};

        impl<'split> Splitable<'split> for tokio::net::TcpStream {
            type ReadHalf = tokio::net::tcp::ReadHalf<'split>;
            type WriteHalf = tokio::net::tcp::WriteHalf<'split>;

            fn split(&'split mut self) -> (Self::ReadHalf, Self::WriteHalf) {
                tokio::net::TcpStream::split(self)
            }
        }
    }
}
