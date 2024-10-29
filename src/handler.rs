use crate::{split, Connection, UnderlyingConnection};
use std::{pin::Pin, future::Future, io::Result as IoResult};

pub type Handler<C> = Box<dyn
    FnOnce(Connection<C>) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>>
    + Send + Sync
>;

pub trait IntoHandler<C: UnderlyingConnection, T> {
    fn into_handler(self) -> Handler<C>;
}

impl<C: UnderlyingConnection> IntoHandler<C, ()> for Handler<C> {
    fn into_handler(self) -> Handler<C> {
        self
    }
}

impl<C: UnderlyingConnection, H, Fut> IntoHandler<C, fn(Connection<C>)> for H
where
    H:   FnOnce(Connection<C>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static
{
    fn into_handler(self) -> Handler<C> {
        Box::new(move |conn| Box::pin({
            let session = self(conn);
            async {session.await}
        }))
    }
}
impl<C: UnderlyingConnection, H, Fut> IntoHandler<C, fn(Connection<C>)->IoResult<()>> for H
where
    H:   FnOnce(Connection<C>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = IoResult<()>> + Send + 'static
{
    fn into_handler(self) -> Handler<C> {
        Box::new(move |conn| Box::pin({
            let session = self(conn);
            async {if let Err(e) = session.await {
                eprintln!("finished WebSocket session: {e}")
            }}
        }))
    }
}

impl<C: UnderlyingConnection, H, Fut> IntoHandler<C, fn(split::ReadHalf<C::ReadHalf>, split::WriteHalf<C::WriteHalf>)> for H
where
    H:   FnOnce(split::ReadHalf<C::ReadHalf>, split::WriteHalf<C::WriteHalf>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static
{
    fn into_handler(self) -> Handler<C> {
        Box::new(move |conn| Box::pin({
            let (r, w) = conn.split();
            let session = self(r, w);
            async {session.await}
        }))
    }
}
impl<C: UnderlyingConnection, H, Fut> IntoHandler<C, fn(split::ReadHalf<C::ReadHalf>, split::WriteHalf<C::WriteHalf>)->IoResult<()>> for H
where
    H:   FnOnce(split::ReadHalf<C::ReadHalf>, split::WriteHalf<C::WriteHalf>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = IoResult<()>> + Send + 'static
{
    fn into_handler(self) -> Handler<C> {
        Box::new(move |conn| Box::pin({
            let (r, w) = conn.split();
            let session = self(r, w);
            async {if let Err(e) = session.await {
                eprintln!("finished WebSocket session: {e}")
            }}
        }))
    }
}

impl<C: UnderlyingConnection, H, Fut> IntoHandler<C, fn(split::WriteHalf<C::WriteHalf>, split::ReadHalf<C::ReadHalf>)> for H
where
    H:   FnOnce(split::WriteHalf<C::WriteHalf>, split::ReadHalf<C::ReadHalf>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static
{
    fn into_handler(self) -> Handler<C> {
        Box::new(move |conn| Box::pin({
            let (r, w) = conn.split();
            let session = self(w, r);
            async {session.await}
        }))
    }
}
impl<C: UnderlyingConnection, H, Fut> IntoHandler<C, fn(split::WriteHalf<C::WriteHalf>, split::ReadHalf<C::ReadHalf>)->IoResult<()>> for H
where
    H:   FnOnce(split::WriteHalf<C::WriteHalf>, split::ReadHalf<C::ReadHalf>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = IoResult<()>> + Send + 'static
{
    fn into_handler(self) -> Handler<C> {
        Box::new(move |conn| Box::pin({
            let (r, w) = conn.split();
            let session = self(w, r);
            async {if let Err(e) = session.await {
                eprintln!("finished WebSocket session: {e}")
            }}
        }))
    }
}