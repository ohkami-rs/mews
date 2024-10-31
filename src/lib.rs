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
//! * Multiple Environment : `tokio`, `async-std`, `smol`, `glommio` are supported as async runtime ( by feature flags of the names ).
//! 
//! ## Note
//! 
//! MEWS is NOT WebSocket server, just protocol implementation. So :
//! 
//! * Tend to be used by web frameworks internally, not by end-developers.
//! 
//! * Doesn't builtins `wss://` support.

#[cfg(any(
    all(feature="tokio", any(feature="async-std",feature="smol",feature="glommio")),
    all(feature="async-std", any(feature="smol",feature="glommio",feature="tokio")),
    all(feature="smol", any(feature="glommio",feature="tokio",feature="async-std")),
    all(feature="glommio", any(feature="tokio", feature="async-std",feature="smol",)),
))]
compile_error! {"More than one runtime feature flags can't be activated"}

#[cfg(feature="__runtime__")]
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

#[cfg(feature="__runtime__")]
pub mod connection;
#[cfg(feature="__runtime__")]
pub mod frame;
#[cfg(feature="__runtime__")]
pub mod message;
#[cfg(feature="__runtime__")]
pub mod websocket;

#[cfg(feature="__runtime__")]
pub use {
    websocket::*,
    connection::Connection,
    connection::split::{self, ReadHalf, WriteHalf},
    frame::CloseCode,
    message::{Message, CloseFrame},
};
