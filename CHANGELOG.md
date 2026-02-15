# smol-axum

This document describes changes between versions of `smol-axum`.

## Version 0.2.1

- **Breaking:** Update to newest `hyper` and add `Sync` bound to input executors.
  As `hyper` does not consider this a breaking change, we do not either.

## Version 0.2.0

- **Breaking:** Update to `axum` version v0.5.
- Remove the dependency on the `tower` crate.

## Version 0.1.0

- Initial release
