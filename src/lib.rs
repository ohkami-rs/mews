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
//! ## Features
//! 
//! * Minimal and Efficient : minimal codebase to provide efficient, memory-safe WebSocket handling.
//! 
//! * Multiple Environment : `tokio`, `smol`, `glommio` are supported as async runtime ( by feature flags `rt_{name}` ).
//! 
//! ## Note
//! 
//! MEWS is NOT WebSocket server, just protocol implementation. So :
//! 
//! * Tend to be used by web frameworks internally, not by end-developers.
//! 
//! * Doesn't builtins `wss://` support.

#[cfg(any(
    all(feature="rt_tokio",   any(feature="rt_smol",    feature="rt_glommio")),
    all(feature="rt_smol",    any(feature="rt_glommio", feature="rt_tokio"  )),
    all(feature="rt_glommio", any(feature="rt_tokio",   feature="rt_smol"   )),
))]
compile_error! {"More than one runtime feature flags can't be activated"}

#[cfg(feature="rt_tokio")]
mod runtime {
    pub use {
        tokio::io::AsyncReadExt as AsyncRead,
        tokio::io::AsyncWriteExt as AsyncWrite,
        tokio::sync::RwLock,
        tokio::time::sleep
    };
    #[cfg(any(test, feature="tcpstream-only"))]
    pub use tokio::net;
}
#[cfg(feature="rt_smol")]
mod runtime {
    pub use {
        futures_util::AsyncReadExt as AsyncRead,
        futures_util::AsyncWriteExt as AsyncWrite,
        smol::lock::RwLock,
    };
    pub async fn sleep(duration: std::time::Duration) {
        smol::Timer::after(duration).await;
    }
    #[cfg(any(test, feature="tcpstream-only"))]
    pub use smol::net;
}
#[cfg(feature="rt_glommio")]
mod runtime {
    pub use {
        futures_util::AsyncReadExt as AsyncRead,
        futures_util::AsyncWriteExt as AsyncWrite,
        glommio::sync::RwLock,
        glommio::timer::sleep
    };
    #[cfg(any(test, feature="tcpstream-only"))]
    pub use glommio::net;
}

pub mod message;
#[cfg(feature="__runtime__")]
pub mod frame;
#[cfg(feature="__runtime__")]
pub mod websocket;
#[cfg(feature="__runtime__")]
pub mod connection;

pub use message::{Message, CloseFrame, CloseCode};
#[cfg(feature="__runtime__")]
pub use {
    websocket::*,
    connection::Connection,
    connection::split::{self, Splitable, ReadHalf, WriteHalf},
};
