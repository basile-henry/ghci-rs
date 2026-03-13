// For macros
#![allow(non_snake_case)]

//! Traits for converting between Rust values and Haskell expressions.
//!
//! [`ToHaskell`] converts a Rust value into a Haskell expression string,
//! and [`FromHaskell`] parses a Haskell expression string back into a Rust value.
//!
//! ```
//! use ghci::{ToHaskell, FromHaskell};
//!
//! assert_eq!(true.to_haskell(), "True");
//! assert_eq!((-3i32).to_haskell(), "(-3)");
//! assert_eq!(Some(42u32).to_haskell(), "(Just 42)");
//! assert_eq!(Vec::<i32>::new().to_haskell(), "[]");
//! assert_eq!(vec![1u32, 2, 3].to_haskell(), "[1, 2, 3]");
//!
//! assert_eq!(bool::from_haskell("True").unwrap(), true);
//! assert_eq!(i32::from_haskell("(-3)").unwrap(), -3);
//! assert_eq!(<Option<u32>>::from_haskell("(Just 42)").unwrap(), Some(42));
//! ```

use std::fmt;

/// Error type for parsing Haskell expressions.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum HaskellParseError {
    /// Failed to parse a Haskell expression.
    #[error("failed to parse Haskell expression: {message}")]
    ParseError {
        /// Description of what went wrong.
        message: String,
    },
    /// Input ended unexpectedly.
    #[error("unexpected end of input")]
    UnexpectedEnd,
    /// Parsing succeeded but there was leftover input.
    #[error("unexpected trailing input: {remaining:?}")]
    TrailingInput {
        /// The unconsumed input.
        remaining: String,
    },
}

/// Convert a Rust value to a Haskell expression string.
///
/// Implementors override [`write_haskell`](ToHaskell::write_haskell).
/// The expression must be safe for use as a function argument
/// (parenthesized where needed, e.g. negative numbers, constructor applications).
pub trait ToHaskell {
    /// Write this value as a Haskell expression into the buffer.
    ///
    /// # Errors
    ///
    /// Returns [`fmt::Error`] if writing to the buffer fails.
    fn write_haskell(&self, buf: &mut impl fmt::Write) -> fmt::Result;

    /// Convenience: return as owned `String`.
    fn to_haskell(&self) -> String {
        let mut buf = String::new();
        self.write_haskell(&mut buf)
            .expect("write to String cannot fail");
        buf
    }
}

/// Parse a Rust value from a Haskell expression string.
///
/// Implementors override [`parse_haskell`](FromHaskell::parse_haskell), which returns the
/// parsed value together with unconsumed input. The default [`from_haskell`](FromHaskell::from_haskell)
/// calls `parse_haskell` and checks that no input remains.
pub trait FromHaskell: Sized {
    /// Parse from full input (checks no trailing content).
    ///
    /// # Errors
    ///
    /// Returns [`HaskellParseError`] if parsing fails or there is trailing input.
    fn from_haskell(input: &str) -> Result<Self, HaskellParseError> {
        let (val, rest) = Self::parse_haskell(input)?;
        let rest = skip_ws(rest);
        if rest.is_empty() {
            Ok(val)
        } else {
            Err(HaskellParseError::TrailingInput {
                remaining: rest.to_string(),
            })
        }
    }

    /// Parse from start of input, return (value, remaining).
    ///
    /// # Errors
    ///
    /// Returns [`HaskellParseError`] if the input cannot be parsed.
    fn parse_haskell(input: &str) -> Result<(Self, &str), HaskellParseError>;
}

// ── ToHaskell impls ──────────────────────────────────────────────────

impl<T: ToHaskell + ?Sized> ToHaskell for &T {
    fn write_haskell(&self, buf: &mut impl fmt::Write) -> fmt::Result {
        (**self).write_haskell(buf)
    }
}

impl ToHaskell for bool {
    fn write_haskell(&self, buf: &mut impl fmt::Write) -> fmt::Result {
        buf.write_str(if *self { "True" } else { "False" })
    }
}

macro_rules! impl_to_haskell_unsigned {
    ($($t:ty),*) => {
        $(impl ToHaskell for $t {
            fn write_haskell(&self, buf: &mut impl fmt::Write) -> fmt::Result {
                write!(buf, "{self}")
            }
        })*
    };
}

impl_to_haskell_unsigned!(u8, u16, u32, u64, u128, usize);

macro_rules! impl_to_haskell_signed {
    ($($t:ty),*) => {
        $(impl ToHaskell for $t {
            fn write_haskell(&self, buf: &mut impl fmt::Write) -> fmt::Result {
                if *self < 0 {
                    write!(buf, "({self})")
                } else {
                    write!(buf, "{self}")
                }
            }
        })*
    };
}

impl_to_haskell_signed!(i8, i16, i32, i64, i128, isize);

macro_rules! impl_to_haskell_float {
    ($($t:ty),*) => {
        $(impl ToHaskell for $t {
            fn write_haskell(&self, buf: &mut impl fmt::Write) -> fmt::Result {
                if self.is_nan() {
                    buf.write_str("(0/0)")
                } else if self.is_infinite() {
                    if self.is_sign_positive() {
                        buf.write_str("(1/0)")
                    } else {
                        buf.write_str("((-1)/0)")
                    }
                } else if *self < 0.0 {
                    write!(buf, "({self:.1})")
                } else {
                    write!(buf, "{self:.1}")
                }
            }
        })*
    };
}

impl_to_haskell_float!(f32, f64);

impl ToHaskell for str {
    fn write_haskell(&self, buf: &mut impl fmt::Write) -> fmt::Result {
        buf.write_char('"')?;
        for c in self.chars() {
            write_haskell_char_escaped(buf, c, '"')?;
        }
        buf.write_char('"')
    }
}

impl ToHaskell for String {
    fn write_haskell(&self, buf: &mut impl fmt::Write) -> fmt::Result {
        self.as_str().write_haskell(buf)
    }
}

impl ToHaskell for char {
    fn write_haskell(&self, buf: &mut impl fmt::Write) -> fmt::Result {
        buf.write_char('\'')?;
        write_haskell_char_escaped(buf, *self, '\'')?;
        buf.write_char('\'')
    }
}

fn write_haskell_char_escaped(buf: &mut impl fmt::Write, c: char, quote: char) -> fmt::Result {
    match c {
        '\\' => buf.write_str("\\\\"),
        '\n' => buf.write_str("\\n"),
        '\t' => buf.write_str("\\t"),
        '\r' => buf.write_str("\\r"),
        '\0' => buf.write_str("\\0"),
        c if c == quote => {
            buf.write_char('\\')?;
            buf.write_char(quote)
        }
        c if c.is_ascii_control() => write!(buf, "\\{}", c as u32),
        c => buf.write_char(c),
    }
}

impl<T: ToHaskell> ToHaskell for Option<T> {
    fn write_haskell(&self, buf: &mut impl fmt::Write) -> fmt::Result {
        match self {
            None => buf.write_str("Nothing"),
            Some(v) => {
                buf.write_str("(Just ")?;
                v.write_haskell(buf)?;
                buf.write_char(')')
            }
        }
    }
}

impl<T: ToHaskell> ToHaskell for Vec<T> {
    fn write_haskell(&self, buf: &mut impl fmt::Write) -> fmt::Result {
        buf.write_char('[')?;
        for (i, v) in self.iter().enumerate() {
            if i > 0 {
                buf.write_str(", ")?;
            }
            v.write_haskell(buf)?;
        }
        buf.write_char(']')
    }
}

impl<T: ToHaskell> ToHaskell for [T] {
    fn write_haskell(&self, buf: &mut impl fmt::Write) -> fmt::Result {
        buf.write_char('[')?;
        for (i, v) in self.iter().enumerate() {
            if i > 0 {
                buf.write_str(", ")?;
            }
            v.write_haskell(buf)?;
        }
        buf.write_char(']')
    }
}

macro_rules! impl_to_haskell_tuple {
    ($($idx:tt $T:ident),+) => {
        impl<$($T: ToHaskell),+> ToHaskell for ($($T,)+) {
            fn write_haskell(&self, buf: &mut impl fmt::Write) -> fmt::Result {
                buf.write_char('(')?;
                let mut _first = true;
                $(
                    if !_first { buf.write_str(", ")?; }
                    _first = false;
                    self.$idx.write_haskell(buf)?;
                )+
                buf.write_char(')')
            }
        }
    };
}

// Tuple implementations for ToHaskell
impl_to_haskell_tuple!(0 A, 1 B);
impl_to_haskell_tuple!(0 A, 1 B, 2 C);
impl_to_haskell_tuple!(0 A, 1 B, 2 C, 3 D);
impl_to_haskell_tuple!(0 A, 1 B, 2 C, 3 D, 4 E);
impl_to_haskell_tuple!(0 A, 1 B, 2 C, 3 D, 4 E, 5 F);
impl_to_haskell_tuple!(0 A, 1 B, 2 C, 3 D, 4 E, 5 F, 6 G);
impl_to_haskell_tuple!(0 A, 1 B, 2 C, 3 D, 4 E, 5 F, 6 G, 7 H);

// ── FromHaskell impls ────────────────────────────────────────────────

impl FromHaskell for bool {
    fn parse_haskell(input: &str) -> Result<(Self, &str), HaskellParseError> {
        let input = skip_ws(input);
        if let Some(rest) = input.strip_prefix("True") {
            if rest.is_empty() || !rest.as_bytes()[0].is_ascii_alphanumeric() {
                return Ok((true, rest));
            }
        }
        if let Some(rest) = input.strip_prefix("False") {
            if rest.is_empty() || !rest.as_bytes()[0].is_ascii_alphanumeric() {
                return Ok((false, rest));
            }
        }
        Err(HaskellParseError::ParseError {
            message: format!("expected True or False, got {input:?}"),
        })
    }
}

/// Parse an integer (possibly parenthesized if negative).
fn parse_int<T: std::str::FromStr>(input: &str) -> Result<(T, &str), HaskellParseError>
where
    T::Err: fmt::Display,
{
    let input = skip_ws(input);
    // Try parenthesized negative: (-123)
    if let Some(inner) = input.strip_prefix('(') {
        let inner = skip_ws(inner);
        if inner.starts_with('-') {
            let end = inner
                .find(')')
                .ok_or_else(|| HaskellParseError::ParseError {
                    message: "unclosed parenthesis for negative number".to_string(),
                })?;
            let num_str = inner[..end].trim();
            let val = num_str
                .parse::<T>()
                .map_err(|e| HaskellParseError::ParseError {
                    message: format!("invalid number {num_str:?}: {e}"),
                })?;
            return Ok((val, &inner[end + 1..]));
        }
    }
    // Bare number
    let end = input
        .find(|c: char| !c.is_ascii_digit() && c != '-')
        .unwrap_or(input.len());
    if end == 0 {
        return Err(HaskellParseError::ParseError {
            message: format!("expected integer, got {input:?}"),
        });
    }
    let num_str = &input[..end];
    let val = num_str
        .parse::<T>()
        .map_err(|e| HaskellParseError::ParseError {
            message: format!("invalid number {num_str:?}: {e}"),
        })?;
    Ok((val, &input[end..]))
}

macro_rules! impl_from_haskell_int {
    ($($t:ty),*) => {
        $(impl FromHaskell for $t {
            fn parse_haskell(input: &str) -> Result<(Self, &str), HaskellParseError> {
                parse_int(input)
            }
        })*
    };
}

impl_from_haskell_int!(u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize);

/// Parse a float (possibly parenthesized if negative, or special values like `(0/0)`).
fn parse_float<T>(input: &str) -> Result<(T, &str), HaskellParseError>
where
    T: std::str::FromStr + From<f32>,
    T::Err: fmt::Display,
{
    let input = skip_ws(input);
    // Handle parenthesized expressions: negative numbers or special values
    if let Some(inner) = input.strip_prefix('(') {
        let inner = skip_ws(inner);
        // (0/0) → NaN
        if let Some(rest) = inner.strip_prefix("0/0)") {
            return Ok((T::from(f32::NAN), rest));
        }
        // (1/0) → Inf
        if let Some(rest) = inner.strip_prefix("1/0)") {
            return Ok((T::from(f32::INFINITY), rest));
        }
        // ((-1)/0) → -Inf
        if let Some(rest) = inner.strip_prefix("(-1)/0)") {
            return Ok((T::from(f32::NEG_INFINITY), rest));
        }
        // Negative float: (-3.0)
        if inner.starts_with('-') {
            let end = inner
                .find(')')
                .ok_or_else(|| HaskellParseError::ParseError {
                    message: "unclosed parenthesis for negative float".to_string(),
                })?;
            let num_str = inner[..end].trim();
            let val = num_str
                .parse::<T>()
                .map_err(|e| HaskellParseError::ParseError {
                    message: format!("invalid float {num_str:?}: {e}"),
                })?;
            return Ok((val, &inner[end + 1..]));
        }
    }
    // Bare positive float
    let end = input
        .find(|c: char| {
            !c.is_ascii_digit() && c != '.' && c != '-' && c != 'e' && c != 'E' && c != '+'
        })
        .unwrap_or(input.len());
    if end == 0 {
        return Err(HaskellParseError::ParseError {
            message: format!("expected float, got {input:?}"),
        });
    }
    let num_str = &input[..end];
    let val = num_str
        .parse::<T>()
        .map_err(|e| HaskellParseError::ParseError {
            message: format!("invalid float {num_str:?}: {e}"),
        })?;
    Ok((val, &input[end..]))
}

impl FromHaskell for f32 {
    fn parse_haskell(input: &str) -> Result<(Self, &str), HaskellParseError> {
        parse_float(input)
    }
}

impl FromHaskell for f64 {
    fn parse_haskell(input: &str) -> Result<(Self, &str), HaskellParseError> {
        parse_float(input)
    }
}

impl FromHaskell for String {
    fn parse_haskell(input: &str) -> Result<(Self, &str), HaskellParseError> {
        let input = skip_ws(input);
        let mut chars = input.chars();
        if chars.next() != Some('"') {
            return Err(HaskellParseError::ParseError {
                message: format!("expected string literal, got {input:?}"),
            });
        }
        let mut result = Self::new();
        let mut rest = &input[1..];
        loop {
            let c = rest
                .chars()
                .next()
                .ok_or(HaskellParseError::UnexpectedEnd)?;
            match c {
                '"' => return Ok((result, &rest[1..])),
                '\\' => {
                    let (escaped, after) = parse_escape(&rest[1..])?;
                    result.push(escaped);
                    rest = after;
                }
                _ => {
                    result.push(c);
                    rest = &rest[c.len_utf8()..];
                }
            }
        }
    }
}

impl FromHaskell for char {
    fn parse_haskell(input: &str) -> Result<(Self, &str), HaskellParseError> {
        let input = skip_ws(input);
        let rest = input
            .strip_prefix('\'')
            .ok_or_else(|| HaskellParseError::ParseError {
                message: format!("expected char literal, got {input:?}"),
            })?;
        let (c, rest) = if let Some(escaped_rest) = rest.strip_prefix('\\') {
            parse_escape(escaped_rest)?
        } else {
            let c = rest
                .chars()
                .next()
                .ok_or(HaskellParseError::UnexpectedEnd)?;
            (c, &rest[c.len_utf8()..])
        };
        if !rest.starts_with('\'') {
            return Err(HaskellParseError::ParseError {
                message: "unterminated char literal".to_string(),
            });
        }
        Ok((c, &rest[1..]))
    }
}

fn parse_escape(input: &str) -> Result<(char, &str), HaskellParseError> {
    let c = input
        .chars()
        .next()
        .ok_or(HaskellParseError::UnexpectedEnd)?;
    match c {
        '\\' => Ok(('\\', &input[1..])),
        '"' => Ok(('"', &input[1..])),
        '\'' => Ok(('\'', &input[1..])),
        'n' => Ok(('\n', &input[1..])),
        't' => Ok(('\t', &input[1..])),
        'r' => Ok(('\r', &input[1..])),
        '0' => Ok(('\0', &input[1..])),
        c if c.is_ascii_digit() => {
            // Numeric escape: \NNN
            let end = input
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(input.len());
            let num: u32 = input[..end]
                .parse()
                .map_err(|_| HaskellParseError::ParseError {
                    message: format!("invalid numeric escape: {}", &input[..end]),
                })?;
            let c = char::from_u32(num).ok_or_else(|| HaskellParseError::ParseError {
                message: format!("invalid unicode code point: {num}"),
            })?;
            Ok((c, &input[end..]))
        }
        _ => Err(HaskellParseError::ParseError {
            message: format!("unknown escape sequence: \\{c}"),
        }),
    }
}

impl<T: FromHaskell> FromHaskell for Option<T> {
    fn parse_haskell(input: &str) -> Result<(Self, &str), HaskellParseError> {
        let input = skip_ws(input);
        if let Some(rest) = input.strip_prefix("Nothing") {
            if rest.is_empty() || !rest.as_bytes()[0].is_ascii_alphanumeric() {
                return Ok((None, rest));
            }
        }
        // (Just expr) — with parens
        if let Some(inner) = input.strip_prefix('(') {
            let inner = skip_ws(inner);
            if let Some(inner) = inner.strip_prefix("Just") {
                if !inner.is_empty() && inner.as_bytes()[0].is_ascii_alphanumeric() {
                    return Err(HaskellParseError::ParseError {
                        message: format!("expected Nothing or Just, got {input:?}"),
                    });
                }
                let (val, rest) = T::parse_haskell(inner)?;
                let rest = skip_ws(rest);
                let rest = rest
                    .strip_prefix(')')
                    .ok_or_else(|| HaskellParseError::ParseError {
                        message: "expected closing ')' for Just".to_string(),
                    })?;
                return Ok((Some(val), rest));
            }
        }
        // Just expr — without parens
        if let Some(inner) = input.strip_prefix("Just") {
            if !inner.is_empty() && inner.as_bytes()[0].is_ascii_alphanumeric() {
                return Err(HaskellParseError::ParseError {
                    message: format!("expected Nothing or Just, got {input:?}"),
                });
            }
            let (val, rest) = T::parse_haskell(inner)?;
            return Ok((Some(val), rest));
        }
        Err(HaskellParseError::ParseError {
            message: format!("expected Nothing or Just, got {input:?}"),
        })
    }
}

impl<T: FromHaskell> FromHaskell for Vec<T> {
    fn parse_haskell(input: &str) -> Result<(Self, &str), HaskellParseError> {
        let input = skip_ws(input);
        let rest = input
            .strip_prefix('[')
            .ok_or_else(|| HaskellParseError::ParseError {
                message: format!("expected '[', got {input:?}"),
            })?;
        let rest = skip_ws(rest);
        if let Some(rest) = rest.strip_prefix(']') {
            return Ok((Self::new(), rest));
        }
        let mut result = Self::new();
        let mut rest = rest;
        loop {
            let (val, r) = T::parse_haskell(rest)?;
            result.push(val);
            let r = skip_ws(r);
            if let Some(r) = r.strip_prefix(']') {
                return Ok((result, r));
            }
            rest = r
                .strip_prefix(',')
                .ok_or_else(|| HaskellParseError::ParseError {
                    message: format!("expected ',' or ']' in list, got {r:?}"),
                })?;
            rest = skip_ws(rest);
        }
    }
}

macro_rules! impl_from_haskell_tuple {
    ($($idx:tt $T:ident),+) => {
        impl<$($T: FromHaskell),+> FromHaskell for ($($T,)+) {
            fn parse_haskell(input: &str) -> Result<(Self, &str), HaskellParseError> {
                let input = skip_ws(input);
                let rest = input.strip_prefix('(').ok_or_else(|| {
                    HaskellParseError::ParseError {
                        message: format!("expected '(' for tuple, got {input:?}"),
                    }
                })?;
                let mut rest = skip_ws(rest);
                let mut _first = true;
                $(
                    if !_first {
                        rest = skip_ws(rest);
                        rest = rest.strip_prefix(',').ok_or_else(|| {
                            HaskellParseError::ParseError {
                                message: format!("expected ',' in tuple, got {rest:?}"),
                            }
                        })?;
                        rest = skip_ws(rest);
                    }
                    _first = false;
                    let ($T, r) = $T::parse_haskell(rest)?;
                    rest = r;
                )+
                let rest = skip_ws(rest);
                let rest = rest.strip_prefix(')').ok_or_else(|| {
                    HaskellParseError::ParseError {
                        message: format!("expected ')' for tuple, got {rest:?}"),
                    }
                })?;
                Ok((($($T,)+), rest))
            }
        }
    };
}

impl_from_haskell_tuple!(0 A, 1 B);
impl_from_haskell_tuple!(0 A, 1 B, 2 C);
impl_from_haskell_tuple!(0 A, 1 B, 2 C, 3 D);
impl_from_haskell_tuple!(0 A, 1 B, 2 C, 3 D, 4 E);
impl_from_haskell_tuple!(0 A, 1 B, 2 C, 3 D, 4 E, 5 F);
impl_from_haskell_tuple!(0 A, 1 B, 2 C, 3 D, 4 E, 5 F, 6 G);
impl_from_haskell_tuple!(0 A, 1 B, 2 C, 3 D, 4 E, 5 F, 6 G, 7 H);

// ── Helper functions ─────────────────────────────────────────────────

/// Skip leading whitespace.
#[must_use]
pub fn skip_ws(input: &str) -> &str {
    input.trim_start()
}

/// Strip a prefix after skipping whitespace, returning the remaining input.
#[must_use]
pub fn consume_prefix<'a>(input: &'a str, prefix: &str) -> Option<&'a str> {
    skip_ws(input).strip_prefix(prefix)
}

/// Parse a constructor name (starts with uppercase), returning `(name, rest)`.
///
/// # Errors
///
/// Returns [`HaskellParseError`] if no constructor is found.
pub fn parse_constructor(input: &str) -> Result<(&str, &str), HaskellParseError> {
    let input = skip_ws(input);
    let first = input
        .chars()
        .next()
        .ok_or(HaskellParseError::UnexpectedEnd)?;
    if !first.is_ascii_uppercase() {
        return Err(HaskellParseError::ParseError {
            message: format!("expected constructor (uppercase), got {input:?}"),
        });
    }
    let end = input
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '\'')
        .unwrap_or(input.len());
    Ok((&input[..end], &input[end..]))
}

/// Parse Haskell record fields: `{field1 = val1, field2 = val2}`.
///
/// Returns `(constructor_already_consumed, fields, rest)` where each field is
/// a `(name, raw_value_string)` pair. The raw value strings preserve nesting.
///
/// # Errors
///
/// Returns [`HaskellParseError`] if the record syntax is malformed.
pub fn parse_record_fields(input: &str) -> Result<(Vec<(&str, &str)>, &str), HaskellParseError> {
    let input = skip_ws(input);
    let rest = input
        .strip_prefix('{')
        .ok_or_else(|| HaskellParseError::ParseError {
            message: format!("expected '{{' for record, got {input:?}"),
        })?;
    let rest = skip_ws(rest);
    if let Some(rest) = rest.strip_prefix('}') {
        return Ok((Vec::new(), rest));
    }

    let mut fields = Vec::new();
    let mut rest = rest;

    loop {
        rest = skip_ws(rest);
        // Parse field name
        let name_end = rest
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '\'')
            .unwrap_or(rest.len());
        if name_end == 0 {
            return Err(HaskellParseError::ParseError {
                message: format!("expected field name, got {rest:?}"),
            });
        }
        let name = &rest[..name_end];
        rest = skip_ws(&rest[name_end..]);

        // Expect '='
        rest = rest
            .strip_prefix('=')
            .ok_or_else(|| HaskellParseError::ParseError {
                message: format!("expected '=' after field name {name:?}, got {rest:?}"),
            })?;
        rest = skip_ws(rest);

        // Parse value: nesting-aware scan until ',' or '}'
        let val_end = find_field_end(rest)?;
        let val = rest[..val_end].trim_end();
        fields.push((name, val));
        rest = &rest[val_end..];
        rest = skip_ws(rest);

        if let Some(r) = rest.strip_prefix('}') {
            return Ok((fields, r));
        }
        rest = rest
            .strip_prefix(',')
            .ok_or_else(|| HaskellParseError::ParseError {
                message: format!("expected ',' or '}}' in record, got {rest:?}"),
            })?;
    }
}

/// Find the end of a record field value, respecting nesting of `()`, `[]`, `{}`,
/// and string literals.
fn find_field_end(input: &str) -> Result<usize, HaskellParseError> {
    let mut depth_paren = 0i32;
    let mut depth_bracket = 0i32;
    let mut depth_brace = 0i32;
    let mut chars = input.char_indices();

    while let Some((i, c)) = chars.next() {
        match c {
            '(' => depth_paren += 1,
            ')' => {
                if depth_paren == 0 {
                    return Ok(i);
                }
                depth_paren -= 1;
            }
            '[' => depth_bracket += 1,
            ']' => {
                if depth_bracket == 0 {
                    return Ok(i);
                }
                depth_bracket -= 1;
            }
            '{' => depth_brace += 1,
            '}' => {
                if depth_brace == 0 {
                    return Ok(i);
                }
                depth_brace -= 1;
            }
            '"' => {
                // Skip string literal
                loop {
                    match chars.next() {
                        Some((_, '"')) => break,
                        Some((_, '\\')) => {
                            chars.next(); // skip escaped char
                        }
                        Some(_) => {}
                        None => {
                            return Err(HaskellParseError::UnexpectedEnd);
                        }
                    }
                }
            }
            '\'' => {
                // Skip char literal
                match chars.next() {
                    Some((_, '\\')) => {
                        chars.next(); // skip escaped char
                    }
                    Some(_) => {}
                    None => return Err(HaskellParseError::UnexpectedEnd),
                }
                // closing quote
                chars.next();
            }
            ',' if depth_paren == 0 && depth_bracket == 0 && depth_brace == 0 => {
                return Ok(i);
            }
            _ => {}
        }
    }

    Ok(input.len())
}

/// Write a Haskell record expression: `TypeName {field1 = val1, field2 = val2}`.
///
/// # Errors
///
/// Returns [`fmt::Error`] if writing to the buffer fails.
pub fn write_haskell_record<T: ToHaskell>(
    buf: &mut impl fmt::Write,
    name: &str,
    fields: &[(&str, T)],
) -> fmt::Result {
    buf.write_str(name)?;
    buf.write_str(" {")?;
    for (i, (field_name, val)) in fields.iter().enumerate() {
        if i > 0 {
            buf.write_str(", ")?;
        }
        buf.write_str(field_name)?;
        buf.write_str(" = ")?;
        val.write_haskell(buf)?;
    }
    buf.write_char('}')
}

/// Write a Haskell constructor application: `(Constructor arg1 arg2)`.
///
/// Parenthesized when there are arguments, bare when there are none.
///
/// # Errors
///
/// Returns [`fmt::Error`] if writing to the buffer fails.
pub fn write_haskell_app<T: ToHaskell>(
    buf: &mut impl fmt::Write,
    constructor: &str,
    args: &[T],
) -> fmt::Result {
    if args.is_empty() {
        buf.write_str(constructor)
    } else {
        buf.write_char('(')?;
        buf.write_str(constructor)?;
        for arg in args {
            buf.write_char(' ')?;
            arg.write_haskell(buf)?;
        }
        buf.write_char(')')
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── ToHaskell tests ──────────────────────────────────────────────

    #[test]
    fn bool_to_haskell() {
        assert_eq!(true.to_haskell(), "True");
        assert_eq!(false.to_haskell(), "False");
    }

    #[test]
    fn unsigned_to_haskell() {
        assert_eq!(42u32.to_haskell(), "42");
        assert_eq!(0u8.to_haskell(), "0");
        assert_eq!(255u8.to_haskell(), "255");
    }

    #[test]
    fn signed_to_haskell() {
        assert_eq!(42i32.to_haskell(), "42");
        assert_eq!((-3i32).to_haskell(), "(-3)");
        assert_eq!(0i32.to_haskell(), "0");
    }

    #[test]
    fn float_to_haskell() {
        assert_eq!(3.0f64.to_haskell(), "3.0");
        assert_eq!((-3.0f64).to_haskell(), "(-3.0)");
        assert_eq!(f64::NAN.to_haskell(), "(0/0)");
        assert_eq!(f64::INFINITY.to_haskell(), "(1/0)");
        assert_eq!(f64::NEG_INFINITY.to_haskell(), "((-1)/0)");
    }

    #[test]
    fn string_to_haskell() {
        assert_eq!("hello".to_haskell(), r#""hello""#);
        assert_eq!("he\"llo".to_haskell(), r#""he\"llo""#);
        assert_eq!("new\nline".to_haskell(), r#""new\nline""#);
        assert_eq!("tab\there".to_haskell(), r#""tab\there""#);
        assert_eq!("back\\slash".to_haskell(), r#""back\\slash""#);
    }

    #[test]
    fn char_to_haskell() {
        assert_eq!('x'.to_haskell(), "'x'");
        assert_eq!('\''.to_haskell(), "'\\''");
        assert_eq!('\n'.to_haskell(), "'\\n'");
    }

    #[test]
    fn option_to_haskell() {
        assert_eq!(None::<i32>.to_haskell(), "Nothing");
        assert_eq!(Some(42u32).to_haskell(), "(Just 42)");
        assert_eq!(Some(-3i32).to_haskell(), "(Just (-3))");
    }

    #[test]
    fn vec_to_haskell() {
        assert_eq!(Vec::<i32>::new().to_haskell(), "[]");
        assert_eq!(vec![1u32, 2, 3].to_haskell(), "[1, 2, 3]");
    }

    #[test]
    fn tuple_to_haskell() {
        assert_eq!((1u32, true).to_haskell(), "(1, True)");
        assert_eq!((1u32, 2u32, 3u32).to_haskell(), "(1, 2, 3)");
    }

    #[test]
    fn nested_to_haskell() {
        assert_eq!(Some(vec![1i32, -2, 3]).to_haskell(), "(Just [1, (-2), 3])");
    }

    #[test]
    fn ref_to_haskell() {
        let x = 42u32;
        assert_eq!((&x).to_haskell(), "42");
    }

    // ── FromHaskell tests ────────────────────────────────────────────

    #[test]
    fn bool_from_haskell() {
        assert_eq!(bool::from_haskell("True").unwrap(), true);
        assert_eq!(bool::from_haskell("False").unwrap(), false);
    }

    #[test]
    fn unsigned_from_haskell() {
        assert_eq!(u32::from_haskell("42").unwrap(), 42);
        assert_eq!(u8::from_haskell("0").unwrap(), 0);
    }

    #[test]
    fn signed_from_haskell() {
        assert_eq!(i32::from_haskell("42").unwrap(), 42);
        assert_eq!(i32::from_haskell("(-3)").unwrap(), -3);
        assert_eq!(i32::from_haskell("0").unwrap(), 0);
    }

    #[test]
    fn float_from_haskell() {
        assert!((f64::from_haskell("3.0").unwrap() - 3.0).abs() < f64::EPSILON);
        assert!((f64::from_haskell("(-3.0)").unwrap() + 3.0).abs() < f64::EPSILON);
        assert!(f64::from_haskell("(0/0)").unwrap().is_nan());
        assert_eq!(f64::from_haskell("(1/0)").unwrap(), f64::INFINITY);
        assert_eq!(f64::from_haskell("((-1)/0)").unwrap(), f64::NEG_INFINITY);
    }

    #[test]
    fn string_from_haskell() {
        assert_eq!(String::from_haskell(r#""hello""#).unwrap(), "hello");
        assert_eq!(String::from_haskell(r#""he\"llo""#).unwrap(), "he\"llo");
        assert_eq!(String::from_haskell(r#""new\nline""#).unwrap(), "new\nline");
        assert_eq!(
            String::from_haskell(r#""back\\slash""#).unwrap(),
            "back\\slash"
        );
    }

    #[test]
    fn char_from_haskell() {
        assert_eq!(char::from_haskell("'x'").unwrap(), 'x');
        assert_eq!(char::from_haskell("'\\''").unwrap(), '\'');
        assert_eq!(char::from_haskell("'\\n'").unwrap(), '\n');
    }

    #[test]
    fn option_from_haskell() {
        assert_eq!(<Option<i32>>::from_haskell("Nothing").unwrap(), None);
        assert_eq!(<Option<u32>>::from_haskell("(Just 42)").unwrap(), Some(42));
        assert_eq!(
            <Option<i32>>::from_haskell("(Just (-3))").unwrap(),
            Some(-3)
        );
        assert_eq!(<Option<u32>>::from_haskell("Just 42").unwrap(), Some(42));
    }

    #[test]
    fn vec_from_haskell() {
        assert_eq!(<Vec<i32>>::from_haskell("[]").unwrap(), Vec::<i32>::new());
        assert_eq!(
            <Vec<u32>>::from_haskell("[1, 2, 3]").unwrap(),
            vec![1, 2, 3]
        );
        assert_eq!(
            <Vec<i32>>::from_haskell("[1, (-2), 3]").unwrap(),
            vec![1, -2, 3]
        );
    }

    #[test]
    fn tuple_from_haskell() {
        assert_eq!(<(u32, bool)>::from_haskell("(1, True)").unwrap(), (1, true));
        assert_eq!(
            <(u32, u32, u32)>::from_haskell("(1, 2, 3)").unwrap(),
            (1, 2, 3)
        );
    }

    #[test]
    fn nested_option_vec_roundtrip() {
        let val: Option<Vec<i32>> = Some(vec![1, -2, 3]);
        let s = val.to_haskell();
        assert_eq!(s, "(Just [1, (-2), 3])");
        let parsed = <Option<Vec<i32>>>::from_haskell(&s).unwrap();
        assert_eq!(parsed, val);
    }

    #[test]
    fn none_roundtrip() {
        let val: Option<i32> = None;
        let s = val.to_haskell();
        let parsed = <Option<i32>>::from_haskell(&s).unwrap();
        assert_eq!(parsed, val);
    }

    #[test]
    fn empty_vec_roundtrip() {
        let val: Vec<i32> = vec![];
        let s = val.to_haskell();
        let parsed = <Vec<i32>>::from_haskell(&s).unwrap();
        assert_eq!(parsed, val);
    }

    #[test]
    fn string_escaping_roundtrip() {
        let val = "hello\n\"world\"\t\\end".to_string();
        let s = val.to_haskell();
        let parsed = String::from_haskell(&s).unwrap();
        assert_eq!(parsed, val);
    }

    #[test]
    fn trailing_input_error() {
        let res = i32::from_haskell("42 extra");
        assert!(matches!(res, Err(HaskellParseError::TrailingInput { .. })));
    }

    // ── Helper function tests ────────────────────────────────────────

    #[test]
    fn test_write_haskell_record() {
        let mut buf = String::new();
        write_haskell_record(&mut buf, "Foo", &[("bar", 42u32)]).unwrap();
        assert_eq!(buf, "Foo {bar = 42}");

        buf.clear();
        write_haskell_record(&mut buf, "Bar", &[("baz", true)]).unwrap();
        assert_eq!(buf, "Bar {baz = True}");
    }

    #[test]
    fn test_write_haskell_app() {
        let mut buf = String::new();
        write_haskell_app::<u32>(&mut buf, "Foo", &[]).unwrap();
        assert_eq!(buf, "Foo");

        buf.clear();
        write_haskell_app(&mut buf, "Foo", &[42u32]).unwrap();
        assert_eq!(buf, "(Foo 42)");

        buf.clear();
        write_haskell_app(&mut buf, "Bar", &[true]).unwrap();
        assert_eq!(buf, "(Bar True)");
    }

    #[test]
    fn test_parse_constructor() {
        let (name, rest) = parse_constructor("Just 42").unwrap();
        assert_eq!(name, "Just");
        assert_eq!(rest, " 42");
    }

    #[test]
    fn test_parse_record_fields() {
        let (fields, rest) = parse_record_fields("{bar = 42, baz = True}").unwrap();
        assert_eq!(fields, vec![("bar", "42"), ("baz", "True")]);
        assert_eq!(rest, "");
    }

    #[test]
    fn test_parse_record_fields_nested() {
        let (fields, rest) = parse_record_fields("{x = (Just [1, 2]), y = \"hello\"}").unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0], ("x", "(Just [1, 2])"));
        assert_eq!(fields[1], ("y", "\"hello\""));
        assert_eq!(rest, "");
    }
}
