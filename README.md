# ghci [![CI Status](https://github.com/basile-henry/ghci-rs/workflows/CI/badge.svg)](https://github.com/basile-henry/ghci-rs/actions) [![crates.io](https://img.shields.io/crates/v/ghci.svg)](https://crates.io/crates/ghci) [![docs.rs](https://docs.rs/ghci/badge.svg)](https://docs.rs/ghci)

A crate to manage and communicate with `ghci` sessions.

```rust
let mut ghci = Ghci::new()?;
let out = ghci.eval("putStrLn \"Hello world\"")?;
assert_eq!(out, "Hello world\n");
```

> **Platform support:** Unix only (Linux, macOS, BSDs).

## Features

### Type conversions

Convert between Rust and Haskell values with `ToHaskell` / `FromHaskell`, and evaluate expressions directly into Rust types with `eval_as`:

```rust
use ghci::{Ghci, ToHaskell, FromHaskell};

let mut ghci = Ghci::new()?;

let x: i32 = ghci.eval_as("1 + 1")?;
assert_eq!(x, 2);

// Built-in support for common types
assert_eq!(true.to_haskell(), "True");
assert_eq!(Some(42u32).to_haskell(), "(Just 42)");
assert_eq!(vec![1u32, 2, 3].to_haskell(), "[1, 2, 3]");
```

### Derive macros

With the `derive` feature, automatically derive conversions for your own types:

```rust
use ghci::{ToHaskell, FromHaskell};

#[derive(ToHaskell, FromHaskell)]
struct Point { x: u32, y: u32 }
// Point { x: 1, y: 2 } <-> "Point {x = 1, y = 2}"

#[derive(ToHaskell, FromHaskell)]
#[haskell(style = "app")]
struct Pair(u32, bool);
// Pair(1, true) <-> "(Pair 1 True)"
```

See the [derive macro docs](https://docs.rs/ghci-derive) for all supported attributes (`name`, `transparent`, `style`, `skip`, `bound`).

### Inline Haskell with `ghci!`

With the `macros` feature, use the `ghci!` macro to write inline Haskell expressions and inject Rust values as let-bindings:

```rust
use ghci::{ghci, Ghci, ToHaskell};

let mut ghci = Ghci::new()?;
ghci.import(&["Data.Char"])?;

// Simple expression
let n: i32 = ghci!(&mut ghci, { length "hello" })?;
assert_eq!(n, 5);

// Inject Rust values as Haskell bindings
let name = "world".to_string();
let greeting: String = ghci!(&mut ghci, [name] { map toUpper name })?;
assert_eq!(greeting, "WORLD");

// Bind with a different Haskell name
let items: Vec<i32> = vec![3, 1, 4, 1, 5];
let sorted: Vec<i32> = ghci!(&mut ghci, [xs = items] { sort xs })?;
assert_eq!(sorted, vec![1, 1, 3, 4, 5]);
```

### Configuration

Use `GhciBuilder` to configure the ghci session:

```rust
use ghci::GhciBuilder;

let mut ghci = GhciBuilder::new()
    .ghci_path("/usr/local/bin/ghci")
    .arg("-XOverloadedStrings")
    .working_dir("/path/to/project")
    .build()?;
```

### Timeouts

```rust
use std::time::Duration;

ghci.set_timeout(Some(Duration::from_secs(5)));
```

### Shared sessions

`SharedGhci` provides a thread-safe, lazily-initialized session for use in tests:

```rust
use ghci::{Ghci, SharedGhci};

static GHCI: SharedGhci = SharedGhci::new(|| {
    let mut ghci = Ghci::new()?;
    ghci.import(&["Data.Char"])?;
    Ok(ghci)
});

let mut ghci = GHCI.lock();
let out = ghci.eval("ord 'A'").unwrap();
assert_eq!(out, "65\n");
```

See the [docs](https://docs.rs/ghci) for the full API.

## License

[MIT License](./LICENSE)

Copyright 2023 Basile Henry
