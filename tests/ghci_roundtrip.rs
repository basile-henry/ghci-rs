use ghci::{FromHaskell, Ghci, SharedGhci, ToHaskell};

static GHCI: SharedGhci = SharedGhci::new(Ghci::new);

/// Helper: check both roundtrip directions through ghci.
///
/// 1. Rust → ghci → Rust: `to_haskell()` → ghci eval (applies `show`) → `from_haskell` → compare
/// 2. ghci → Rust → ghci: take ghci's `show` output → `from_haskell` → `to_haskell` → ghci eval → compare with original ghci output
fn roundtrip<T>(value: T)
where
    T: ToHaskell + FromHaskell + PartialEq + std::fmt::Debug,
{
    let haskell_expr = value.to_haskell();
    let mut ghci = GHCI.lock();

    // Direction 1: Rust → ghci → Rust
    let shown = ghci.eval(&haskell_expr).unwrap_or_else(|e| {
        panic!("ghci eval failed for {haskell_expr:?}: {e}");
    });
    let shown = shown.trim();
    let parsed = T::from_haskell(shown).unwrap_or_else(|e| {
        panic!("FromHaskell failed for ghci output {shown:?} (from {haskell_expr:?}): {e}");
    });
    assert_eq!(
        parsed, value,
        "Rust→ghci→Rust mismatch: {haskell_expr:?} -> {shown:?} -> {parsed:?}"
    );

    // Direction 2: ghci → Rust → ghci
    // Re-encode the parsed value and evaluate again — ghci's show output should be stable
    let re_encoded = parsed.to_haskell();
    let shown2 = ghci.eval(&re_encoded).unwrap_or_else(|e| {
        panic!("ghci eval failed for re-encoded {re_encoded:?}: {e}");
    });
    assert_eq!(
        shown2.trim(),
        shown,
        "ghci→Rust→ghci mismatch: {shown:?} -> {re_encoded:?} -> {:?}",
        shown2.trim()
    );
}

/// Helper: evaluate a Haskell expression in ghci, parse the shown output, and
/// compare against an expected Rust value.
fn eval_roundtrip<T>(expr: &str, expected: T)
where
    T: FromHaskell + PartialEq + std::fmt::Debug,
{
    let mut ghci = GHCI.lock();
    let shown = ghci.eval(expr).unwrap_or_else(|e| {
        panic!("ghci eval failed for {expr:?}: {e}");
    });
    let parsed = T::from_haskell(shown.trim()).unwrap_or_else(|e| {
        panic!("FromHaskell failed for ghci output {shown:?} (from {expr:?}): {e}");
    });
    assert_eq!(
        parsed, expected,
        "eval_roundtrip mismatch for {expr:?}: {shown:?} -> {parsed:?}"
    );
}

// ── Escape sequences ────────────────────────────────────────────────

#[test]
fn control_char_escapes() {
    let mut ghci = GHCI.lock();
    for code_point in (0u32..=31).chain(std::iter::once(127)) {
        let expr = format!("toEnum {code_point} :: Char");
        let parsed: char = ghci.eval_as(&expr).unwrap_or_else(|e| {
            panic!("failed to parse ghci output for code point {code_point}: {e}")
        });
        assert_eq!(
            parsed as u32, code_point,
            "code point {code_point}: roundtrip mismatch"
        );
    }
}

#[test]
fn string_with_escapes_roundtrip() {
    roundtrip("hello\nworld".to_string());
    roundtrip("tab\there".to_string());
    roundtrip("back\\slash".to_string());
    roundtrip("quote\"inside".to_string());
    roundtrip("null\0char".to_string());
    roundtrip("\x07bell".to_string());
}

// ── Booleans ────────────────────────────────────────────────────────

#[test]
fn bool_roundtrip() {
    roundtrip(true);
    roundtrip(false);
}

// ── Integers ────────────────────────────────────────────────────────

#[test]
fn unsigned_roundtrip() {
    roundtrip(0u32);
    roundtrip(42u32);
    roundtrip(255u8);
    roundtrip(u64::MAX);
}

#[test]
fn signed_roundtrip() {
    roundtrip(0i32);
    roundtrip(42i32);
    roundtrip(-3i32);
    roundtrip(i64::MIN);
    roundtrip(i64::MAX);
}

#[test]
fn negative_number_formatting() {
    // Verify ghci accepts our parenthesized negatives
    eval_roundtrip::<i32>("(-3)", -3);
    eval_roundtrip::<i32>("(-127)", -127);
    // Verify ghci's show output for negatives can be parsed
    eval_roundtrip::<i32>("negate 42", -42);
}

// ── Floats ──────────────────────────────────────────────────────────

#[test]
fn float_precision_roundtrip() {
    roundtrip(0.0f64);
    roundtrip(1.0f64);
    roundtrip(-1.0f64);
    roundtrip(std::f64::consts::PI);
    roundtrip(-0.001f64);
    roundtrip(1e10f64);
    roundtrip(1.23e-4f64);
}

#[test]
fn float_special_values() {
    // NaN can't use roundtrip (NaN != NaN), test manually
    let mut ghci = GHCI.lock();
    let nan_expr = f64::NAN.to_haskell();
    let shown = ghci.eval(&nan_expr).unwrap();
    assert_eq!(shown.trim(), "NaN");

    let inf_expr = f64::INFINITY.to_haskell();
    let shown = ghci.eval(&inf_expr).unwrap();
    assert_eq!(shown.trim(), "Infinity");

    let neg_inf_expr = f64::NEG_INFINITY.to_haskell();
    let shown = ghci.eval(&neg_inf_expr).unwrap();
    assert_eq!(shown.trim(), "-Infinity");
}

#[test]
fn float_f32_roundtrip() {
    roundtrip(0.0f32);
    roundtrip(1.0f32);
    roundtrip(-1.5f32);
    roundtrip(std::f32::consts::PI);
}

// ── Strings and chars ───────────────────────────────────────────────

#[test]
fn string_roundtrip() {
    roundtrip(String::new());
    roundtrip("hello world".to_string());
    roundtrip("with spaces and 123 numbers".to_string());
}

#[test]
fn char_roundtrip() {
    roundtrip('x');
    roundtrip(' ');
    roundtrip('\'');
    roundtrip('\\');
    roundtrip('"');
    roundtrip('\n');
}

// ── Option ──────────────────────────────────────────────────────────

#[test]
fn option_roundtrip() {
    roundtrip(None::<i32>);
    roundtrip(Some(42i32));
    roundtrip(Some(-3i32));
    roundtrip(Some("hello".to_string()));
}

// ── Vec ─────────────────────────────────────────────────────────────

#[test]
fn vec_roundtrip() {
    roundtrip(Vec::<i32>::new());
    roundtrip(vec![1i32, 2, 3]);
    roundtrip(vec![-1i32, 0, 1]);
    roundtrip(vec!["hello".to_string(), "world".to_string()]);
}

// ── Tuples ──────────────────────────────────────────────────────────

#[test]
fn tuple_roundtrip() {
    roundtrip((1u32, true));
    roundtrip((1u32, 2u32, 3u32));
    roundtrip(("hello".to_string(), 42i32));
    roundtrip((true, false, true, false));
}

// ── Nested types ────────────────────────────────────────────────────

#[test]
fn nested_roundtrip() {
    roundtrip(Some(vec![1i32, -2, 3]));
    roundtrip(vec![Some(1i32), None, Some(3)]);
    roundtrip(vec![(1u32, true), (2, false)]);
    roundtrip(Some((42i32, "hello".to_string())));
}
