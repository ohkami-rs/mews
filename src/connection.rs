use crate::{Config, Message};
use crate::runtime::{Read, Write};
use std::{sync::Arc, cell::UnsafeCell};
use std::io::{Error, ErrorKind};

pub struct Connection<C: Read + Write + Unpin> {
    conn:       Arc<UnsafeCell<Option<C>>>,
    config:     Config,
    n_buffered: usize,
}

/* this requires `Arc<{C with inner mutability}>` */
impl<C: Read + Write + Unpin> Clone for Connection<C> {
    fn clone(&self) -> Self {
        Self {
            conn:       self.conn.clone(),
            config:     self.config.clone(),
            n_buffered: self.n_buffered.clone()
        }
    }
}

unsafe impl<C: Read + Write + Unpin> Send for Connection<C> {}
unsafe impl<C: Read + Write + Unpin> Sync for Connection<C> {}

const _: () = {
    impl<C: Read + Write + Unpin + std::fmt::Debug> std::fmt::Debug for Connection<C> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("WebSocket Connection")
                .field("underlying", unsafe {&*self.conn.get()})
                .field("config", &self.config)
                .field("n_buffered", &self.n_buffered)
                .finish()
        }
    }

    impl<C: Read + Write + Unpin> Connection<C> {
        pub(crate) fn new(conn: C, config: Config) -> Self {
            let conn = Arc::new(UnsafeCell::new(Some(conn)));
            Self { conn, config, n_buffered: 0 }
        }

        pub(crate) fn close(&mut self) {
            // SAFETY: `&mut self` has unique access to `self.conn`
            *unsafe {&mut *self.conn.get()} = None
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
    ($this:ident) => {
        {&mut *$this.conn.get()}.as_mut().ok_or_else(|| {
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
            Error::new(ErrorKind::ConnectionReset, "WebSocket connection is already closed")
        })?
    };
}

impl<C: Read + Write + Unpin> Connection<C> {
    pub fn is_closed(&self) -> bool {
        unsafe {&*self.conn.get()}.is_none()
    }

    pub async fn recv(&mut self) -> Result<Option<Message>, Error> {
        // SAFETY: `&mut self` has unique access to `self.conn`
        let conn = unsafe {underlying!(self)};

        let message = Message::read_from(conn, &self.config).await?;
        if let Some(Message::Ping(payload)) = &message {
            self.send(Message::Pong(payload.clone())).await?
        }

        Ok(message)
    }

    pub async fn send(&mut self, message: Message) -> Result<(), Error> {
        // SAFETY: `&mut self` has unique access to `self.conn`
        let conn = unsafe {underlying!(self)};

        let closing = matches!(message, Message::Close(_));
        send(message, conn, &self.config, &mut self.n_buffered).await?;
        if closing {self.close()}
        Ok(())
    }

    pub async fn write(&mut self, message: Message) -> Result<usize, Error> {
        // SAFETY: `&mut self` has unique access to `self.conn`
        let conn = unsafe {underlying!(self)};

        let closing = matches!(message, Message::Close(_));
        let n = write(message, conn, &self.config, &mut self.n_buffered).await?;
        if closing {self.close()}
        Ok(n)
    }

    pub async fn flush(&mut self) -> Result<(), Error> {
        // SAFETY: `&mut self` has unique access to `self.conn`
        let conn = unsafe {underlying!(self)};

        flush(conn, &mut self.n_buffered).await
    }
}

#[cfg(feature="tokio")]
const _: (/* split on tokio */) = {
    
};
