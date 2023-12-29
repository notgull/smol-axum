// MIT/Apache2 License

//! Hello world example.

use async_io::Async;
use axum::{response::Html, routing::get, Router};
use macro_rules_attribute::apply;

use std::io;
use std::net::TcpListener;
use std::sync::Arc;

#[apply(smol_macros::main!)]
async fn main(ex: &Arc<smol_macros::Executor<'_>>) -> io::Result<()> {
    // build our application with a route
    let app = Router::new().route("/", get(handler));

    // run it
    let listener = Async::<TcpListener>::bind(([127, 0, 0, 1], 3000)).unwrap();
    println!("listening on {}", listener.get_ref().local_addr().unwrap());
    smol_axum::serve(ex.clone(), listener, app).await
}

async fn handler() -> Html<&'static str> {
    Html("<h1>Hello, World!</h1>")
}
