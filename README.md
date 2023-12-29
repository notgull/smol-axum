# smol-axum

Integrations between [`smol`] and [`axum`].

By default, [`axum`] only supports the [`tokio`] runtime. This crate adds a `serve`
function that can be used with [`smol`]'s networking types.

## Examples

```rust
use async_io::Async;
use axum::{response::Html, routing::get, Router};
use macro_rules_attribute::apply;

use std::io;
use std::net::TcpListener;
use std::sync::Arc;

#[apply(smol_macros::main!)]
async fn main(ex: &Arc<smol_macros::Executor<'_>>) -> io::Result<()> {
    // Build our application with a route.
    let app = Router::new().route("/", get(handler));

    // Create a `smol`-based TCP listener.
    let listener = Async::<TcpListener>::bind(([127, 0, 0, 1], 3000)).unwrap();
    println!("listening on {}", listener.get_ref().local_addr().unwrap());

    // Run it using `smol_axum`
    smol_axum::serve(ex.clone(), listener, app).await
}

async fn handler() -> Html<&'static str> {
    Html("<h1>Hello, World!</h1>")
}
```

[`axum`]: https://crates.io/crates/axum
[`smol`]: https://crates.io/crates/smol
[`tokio`]: https://crates.io/crates/tokio

## License

Licensed under either of

 * Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

#### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
