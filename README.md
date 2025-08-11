<div align="center">
    <h1>MEWS</h1>
    Minimal and Efficient, Multi-Environment WebSocket implementation for async Rust
</div>

<br>

<div align="right">
    <a href="https://github.com/ohkami-rs/mews/blob/main/LICENSE"><img alt="License" src="https://img.shields.io/crates/l/mews.svg" /></a>
    <a href="https://github.com/ohkami-rs/mews/actions"><img alt="CI status" src="https://github.com/ohkami-rs/mews/actions/workflows/CI.yml/badge.svg"/></a>
    <a href="https://crates.io/crates/mews"><img alt="crates.io" src="https://img.shields.io/crates/v/mews" /></a>
</div>

## Features

* Minimal and Efficient : minimal codebase to provide efficient, memory-safe WebSocket handling.

* Multi Environment : `tokio`, `smol`, `glommio` are supported as async runtime ( by feature flags `rt_{name}` ).

## Note

MEWS is NOT WebSocket server, just protocol implementation. So :

* Tend to be used by web libraries internally, not by end-developers.

* Doesn't builtins `wss://` support.

## Example

```toml
[dependencies]
mews  = { version = "0.2", features = ["rt_tokio"] }
tokio = { version = "1",   features = ["rt"] }
# ...
```
*( with pseudo Request & Response )*
```rust
/* server */

use mews::{WebSocketContext, Connection, Message};

async fn handle_websocket(
    req: &Request/* upgrade request */,
    tcp: TcpStream
) {
    let ctx = WebSocketContext::new(
        &req.headers["Sec-WebSocket-Key"]
    );

    let (sign, ws) = ctx.on_upgrade(
        |mut conn: Connection| async move {
            while let Ok(Some(Message::Text(text))) = conn.recv().await {
                conn.send(text).await
                    .expect("failed to send message");
                sleep(Duration::from_secs(1)).await;
            }
        }
    );

    send(Response::SwitchingProtocol()
        .with(Connection, "Upgrade")
        .with(Upgrade, "websocket")
        .with(SecWebSocketAccept, sign),
        &mut tcp
    ).await.expect("failed to send handshake response");

    ws.manage(tcp);
}
```
```rust
/* client */

async fn start_websocket(
    mut tcp: TcpStream
) {
    let websocket_key = "my-sec-websocket-key";

    let ctx = WebSocketContext::new(
        websocket_key
    );

    let (sign, ws) = ctx.on_upgrade(
        |mut conn: Connection| async move {
            conn.send("Hello!").await.expect("failed to send message");
            while let Ok(Some(Message::Text(text))) = conn.recv().await {
                println!("got: `{text}`")
            }
        }
    );

    let res = send(Request::GET("/ws")
        .with(Host, "localhost:3000")
        .with(Connection, Upgrade)
        .with(Upgrade, "websocket")
        .with(SecWebSocketVersion, "13")
        .with(SecWebSocketKey, websocket_key),
        &mut tcp
    ).await.expect("failed to send handshake request");

    assert!(res.header(SecWebSocketAccept), Some(sign));

    ws.manage(tcp);
}
```

## License

MEWS is licensed under MIT LICENSE ( [LICENSE](https://github.com/ohkami-rs/mews/blob/main/LICENSE) or [https://opensource.org/licenses/MIT](https://opensource.org/licenses/MIT) ).
