#[cfg(not(any(feature="tokio", feature="async-std", feature="smol", feature="glommio")))]
compile_error! {"One feature flag must be activated"}

#[cfg(any(
    all(feature="tokio", any(feature="async-std",feature="smol",feature="glommio")),
    all(feature="async-std", any(feature="smol",feature="glommio",feature="tokio")),
    all(feature="smol", any(feature="glommio",feature="tokio",feature="async-std")),
    all(feature="glommio", any(feature="tokio", feature="async-std",feature="smol",)),
))]
compile_error! {"More than one feature flags can't be activated"}

mod runtime {
    #[cfg(feature="tokio")]
    pub use {
        tokio::net::TcpStream,
        tokio::io::AsyncReadExt as Read,
        tokio::io::AsyncWriteExt as Write,
    };

    #[cfg(feature="async-std")]
    pub use {
        async_std::net::TcpStream,
        async_std::io::ReadExt as Read,
        async_std::io::WriteExt as Write,
    };

    #[cfg(feature="smol")]
    pub use {
        smol::net::TcpStream,
        smol::io::AsyncReadExt,
        smol::io::AsyncWriteExt,
    };

    #[cfg(feature="glommio")]
    pub use {
        glommio::net::TcpStream,
        futures_util::AsyncReadExt,
        futures_util::AsyncWriteExt,
    };
}

mod connection;
mod frame;
mod message;

pub use connection::Connection;
pub use message::{Message, CloseFrame};
pub use frame::CloseCode;

///////////////////////////////////////////////////////////////////////////////

use crate::runtime::{Read, Write, TcpStream};
use std::{pin::Pin, future::Future};

pub struct WebSocket<C: Read + Write + Unpin = TcpStream> {
    pub sec_websocket_key: String,
    pub config:            Config,
    pub handler:           Handler<C>,
    _priv: ()
}

pub type Handler<C> = Box<dyn
    FnOnce(Connection<C>) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>>
    + Send + Sync
>;

/// ## Note
/// 
/// - Currently, subprotocols via `Sec-WebSocket-Protocol` is not supported
#[derive(Clone, Debug)]
pub struct Config {
    pub write_buffer_size:      usize,
    pub max_write_buffer_size:  usize,
    pub accept_unmasked_frames: bool,
    pub max_message_size:       Option<usize>,
    pub max_frame_size:         Option<usize>,
} const _: () = {
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
