# Changelog

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
