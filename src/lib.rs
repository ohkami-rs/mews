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

#[cfg(feature="__io__")]
mod io {
    #[cfg(feature="io_tokio")]
    pub(crate) use tokio::io::{AsyncReadExt as AsyncRead, AsyncWriteExt as AsyncWrite};
    #[cfg(feature="io_futures")]
    pub(crate) use futures_util::io::{AsyncReadExt as AsyncRead, AsyncWriteExt as AsyncWrite};
}

#[cfg(feature="__io__")]
mod sync {
    pub(crate) use tokio::sync::RwLock;
}

#[cfg(feature="__io__")]
pub mod message;
#[cfg(feature="__io__")]
pub mod frame;
#[cfg(feature="__io__")]
pub mod websocket;
#[cfg(feature="__io__")]
pub mod connection;

#[cfg(feature="__io__")]
pub use {
    message::{Message, CloseFrame, CloseCode},
    websocket::*,
    connection::{Connection, split::{self, Splitable, ReadHalf, WriteHalf}},
};
