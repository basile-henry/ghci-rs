# ghci [![CI Status](https://github.com/basile-henry/ghci-rs/workflows/CI/badge.svg)](https://github.com/basile-henry/ghci-rs/actions) [![crates.io](https://img.shields.io/crates/v/ghci.svg)](https://crates.io/crates/ghci) [![docs.rs](https://docs.rs/ghci/badge.svg)](https://docs.rs/ghci)

A crate to manage and communicate with `ghci` sessions.

```rust
let mut ghci = Ghci::new()?;
let out = ghci.eval("putStrLn \"Hello world\"")?;
assert_eq!(out, "Hello world\n");
```

See the [docs](https://docs.rs/ghci) for more.

> **Platform support:** Unix only (Linux, macOS, BSDs).

## License

[MIT License](./LICENSE)

Copyright 2023 Basile Henry
