# Changelog

## [0.2.1] - 2026/03/15

### Fixed

- Float `ToHaskell` no longer truncates to 1 decimal place — full precision is preserved.
- `Drop` impl no longer panics if the child process is in an unexpected state.
- `clear_blocking_reader_until` no longer panics when ghci startup output exceeds 1024 bytes.
- `FromHaskell` now handles all Haskell escape sequences (`\a`, `\b`, `\f`, `\v`, named escapes like `\NUL`, `\SOH`, `\DEL`, and the `\&` empty escape for disambiguation).

### Added

- `ghci-derive` crate with derive macros for `ToHaskell` and `FromHaskell` traits (enable via the `derive` feature).
- `SharedGhci::try_lock()` — fallible alternative to `lock()` that returns `Result` instead of panicking on mutex poisoning.

## [0.2.0] - 2026/03/14

### Breaking changes

- `eval()` now returns `Result<String>` (stdout only) and returns `Err(GhciError::EvalError { .. })` when ghci produces stderr. Use `eval_raw()` to get both stdout and stderr as before.
- New error variants: `EvalError`, `HaskellParse`, `DisallowedInput`.
- `~/.ghci` is now ignored by default to ensure a consistent prompt. Pass `-ghci-script` via `GhciBuilder` if needed.

### Migration from 0.1.0

```rust
// 0.1.0
let out = ghci.eval("1 + 1")?;        // out: EvalOutput
println!("{}", out.stdout);

// 0.2.0
let out = ghci.eval("1 + 1")?;        // out: String
println!("{}", out);

// To get both stdout and stderr (old behaviour):
let out = ghci.eval_raw("1 + 1")?;    // out: EvalOutput
println!("{} {}", out.stdout, out.stderr);
```

### Added

- `GhciBuilder` for configuring ghci path, arguments, and working directory.
- `eval_raw()` for getting both stdout and stderr without error on stderr.
- `eval_as::<T>()` for evaluating and parsing results via `FromHaskell`.
- `ToHaskell` / `FromHaskell` traits for converting between Rust values and Haskell expressions (primitives, `Option`, `Vec`, tuples, records, constructor applications).
- `SharedGhci` for sharing a session across threads (useful for test suites).
- Deadline-based eval timeout.
- `:set prompt` input rejection to prevent prompt desync.

## [0.1.0] - 2023

Initial release.
