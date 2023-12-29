// MIT/Apache2 License

//! Integrations between [`smol`] and [`axum`].
//!
//! By default, [`axum`] only supports the [`tokio`] runtime. This crate adds a `serve`
//! function that can be used with [`smol`]'s networking types.
//!
//! ## Examples
//!
//! ```no_run
//! use async_io::Async;
//! use axum::{response::Html, routing::get, Router};
//! use macro_rules_attribute::apply;
//!
//! use std::io;
//! use std::net::TcpListener;
//! use std::sync::Arc;
//!
//! #[apply(smol_macros::main!)]
//! async fn main(ex: &Arc<smol_macros::Executor<'_>>) -> io::Result<()> {
//!     // Build our application with a route.
//!     let app = Router::new().route("/", get(handler));
//!
//!     // Create a `smol`-based TCP listener.
//!     let listener = Async::<TcpListener>::bind(([127, 0, 0, 1], 3000)).unwrap();
//!     println!("listening on {}", listener.get_ref().local_addr().unwrap());
//!
//!     // Run it using `smol_axum`
//!     smol_axum::serve(ex.clone(), listener, app).await
//! }
//!
//! async fn handler() -> Html<&'static str> {
//!     Html("<h1>Hello, World!</h1>")
//! }
//! ```
//!
//! [`axum`]: https://crates.io/crates/axum
//! [`smol`]: https://crates.io/crates/smol
//! [`tokio`]: https://crates.io/crates/tokio

#![forbid(unsafe_code)]

use async_executor::Executor;
use async_io::Async;
use hyper::body::Incoming as HyperIncoming;
use hyper_util::server::conn::auto::Builder;
use pin_project_lite::pin_project;
use smol_hyper::rt::{FuturesIo, SmolExecutor, SmolTimer};
use tower::util::{Oneshot, ServiceExt};
use tower_service::Service;

use axum_core::body::Body;
use axum_core::extract::Request;
use axum_core::response::Response;

use futures_lite::future::poll_fn;
use futures_lite::io::{AsyncRead, AsyncWrite};

use std::borrow::Borrow;
use std::convert::Infallible;
use std::future::Future;
use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::pin::Pin;
use std::task::{Context, Poll};

/// Something that produces incoming connections.
pub trait Incoming {
    /// The resulting connections.
    type Connection: AsyncRead + AsyncWrite;

    /// Future for accepting a new connection.
    type Accept<'a>: Future<Output = io::Result<Option<(Self::Connection, SocketAddr)>>> + 'a
    where
        Self: 'a;

    /// Wait for a new connection to arrive.
    fn accept(&self) -> Self::Accept<'_>;
}

impl<'this, T: Incoming + ?Sized> Incoming for &'this T {
    type Accept<'a> = T::Accept<'a> where 'this: 'a;
    type Connection = T::Connection;

    #[inline]
    fn accept(&self) -> Self::Accept<'_> {
        (**self).accept()
    }
}

impl<'this, T: Incoming + ?Sized> Incoming for &'this mut T {
    type Accept<'a> = T::Accept<'a> where 'this: 'a;
    type Connection = T::Connection;

    #[inline]
    fn accept(&self) -> Self::Accept<'_> {
        (**self).accept()
    }
}

impl<T: Incoming + ?Sized> Incoming for Box<T> {
    type Accept<'a> = T::Accept<'a> where T: 'a;
    type Connection = T::Connection;

    #[inline]
    fn accept(&self) -> Self::Accept<'_> {
        (**self).accept()
    }
}

impl Incoming for Async<TcpListener> {
    type Accept<'a> = Pin<
        Box<dyn Future<Output = io::Result<Option<(Self::Connection, SocketAddr)>>> + Send + 'a>,
    >;
    type Connection = Async<TcpStream>;

    #[inline]
    fn accept(&self) -> Self::Accept<'_> {
        Box::pin(async move { self.accept().await.map(Some) })
    }
}

#[cfg(feature = "async-net")]
impl Incoming for async_net::TcpListener {
    type Accept<'a> = Pin<
        Box<dyn Future<Output = io::Result<Option<(Self::Connection, SocketAddr)>>> + Send + 'a>,
    >;
    type Connection = async_net::TcpStream;

    #[inline]
    fn accept(&self) -> Self::Accept<'_> {
        Box::pin(async move { self.accept().await.map(Some) })
    }
}

/// Serve a future using [`smol`]'s TCP listener.
pub async fn serve<'ex, I, S>(
    executor: impl Borrow<Executor<'ex>> + Clone + Send + 'ex,
    tcp_listener: I,
    service: S,
) -> io::Result<()>
where
    I: Incoming + 'static,
    I::Connection: Send + Unpin,
    S: Service<Request, Response = Response, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send,
{
    loop {
        // Wait for a new connection.
        let (tcp_stream, _remote_addr) = match tcp_listener.accept().await? {
            Some(conn) => conn,
            None => break,
        };

        // Wrap it in a `FuturesIo`.
        let tcp_stream = FuturesIo::new(tcp_stream);

        // Wait for the service to be ready.
        let mut service = service.clone();
        poll_fn(|cx| service.poll_ready(cx))
            .await
            .unwrap_or_else(|e| match e {});

        // Create a service.
        let service = { TowerToHyperService { service } };

        // Spawn the service on our executor.
        let task = executor.borrow().spawn({
            let executor = executor.clone();
            async move {
                let mut builder = Builder::new(SmolExecutor::new(AsRefExecutor(executor.borrow())));
                builder.http1().timer(SmolTimer::new());
                builder.http2().timer(SmolTimer::new());

                if let Err(err) = builder
                    .serve_connection_with_upgrades(tcp_stream, service)
                    .await
                {
                    tracing::error!("unintelligible hyper error: {err}");
                }
            }
        });

        // Detach the task and let it run forever.
        task.detach();
    }

    Ok(())
}

/// Convert a Tower service to the Hyper service.
#[derive(Debug, Copy, Clone)]
struct TowerToHyperService<S> {
    service: S,
}

impl<S> hyper::service::Service<Request<HyperIncoming>> for TowerToHyperService<S>
where
    S: tower_service::Service<Request> + Clone,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = TowerToHyperServiceFuture<S, Request>;

    fn call(&self, req: Request<HyperIncoming>) -> Self::Future {
        let req = req.map(Body::new);
        TowerToHyperServiceFuture {
            future: self.service.clone().oneshot(req),
        }
    }
}

pin_project! {
    struct TowerToHyperServiceFuture<S, R>
    where
        S: tower_service::Service<R>,
    {
        #[pin]
        future: Oneshot<S, R>,
    }
}

impl<S, R> Future for TowerToHyperServiceFuture<S, R>
where
    S: tower_service::Service<R>,
{
    type Output = Result<S::Response, S::Error>;

    #[inline]
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.project().future.poll(cx)
    }
}

#[derive(Clone)]
struct AsRefExecutor<'this, 'ex>(&'this Executor<'ex>);

impl<'ex> AsRef<Executor<'ex>> for AsRefExecutor<'_, 'ex> {
    #[inline]
    fn as_ref(&self) -> &Executor<'ex> {
        self.0
    }
}
