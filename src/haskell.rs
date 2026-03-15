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
fn skip_ws(input: &str) -> &str {
    input.trim_start()
}

fn parse_constructor(input: &str) -> Result<(&str, &str), HaskellParseError> {
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

fn parse_identifier(input: &str) -> Result<(&str, &str), HaskellParseError> {
    let input = skip_ws(input);
    let first = input
        .chars()
        .next()
        .ok_or(HaskellParseError::UnexpectedEnd)?;
    if !first.is_ascii_alphabetic() && first != '_' {
        return Err(HaskellParseError::ParseError {
            message: format!("expected identifier, got {input:?}"),
        });
    }
    let end = input
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '\'')
        .unwrap_or(input.len());
    Ok((&input[..end], &input[end..]))
}

#[allow(clippy::type_complexity)]
fn parse_record_fields(input: &str) -> Result<(Vec<(&str, &str)>, &str), HaskellParseError> {
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

// ── ToHaskell builders ────────────────────────────────────────────────

/// Builder for writing Haskell record expressions: `Name {f1 = v1, f2 = v2}`.
///
/// Created by [`record`]. Each [`field`](HaskellRecord::field) call can use a
/// different `ToHaskell` type.
pub struct HaskellRecord<'a, W: fmt::Write> {
    buf: &'a mut W,
    result: fmt::Result,
    has_fields: bool,
}

/// Start writing a Haskell record expression.
///
/// ```
/// use ghci::haskell;
/// use ghci::ToHaskell;
///
/// let mut buf = String::new();
/// haskell::record(&mut buf, "Point")
///     .field("x", &1u32)
///     .field("y", &2u32)
///     .finish()
///     .unwrap();
/// assert_eq!(buf, "Point {x = 1, y = 2}");
/// ```
pub fn record<'a, W: fmt::Write>(buf: &'a mut W, name: &str) -> HaskellRecord<'a, W> {
    let result = buf.write_str(name).and_then(|()| buf.write_str(" {"));
    HaskellRecord {
        buf,
        result,
        has_fields: false,
    }
}

impl<W: fmt::Write> HaskellRecord<'_, W> {
    /// Write a field `name = value`. Each call may use a different `ToHaskell` type.
    pub fn field<T: ToHaskell>(&mut self, name: &str, value: &T) -> &mut Self {
        self.result = self.result.and_then(|()| {
            if self.has_fields {
                self.buf.write_str(", ")?;
            }
            self.buf.write_str(name)?;
            self.buf.write_str(" = ")?;
            value.write_haskell(self.buf)
        });
        self.has_fields = true;
        self
    }

    /// Finish the record expression by writing `}`.
    ///
    /// # Errors
    ///
    /// Returns [`fmt::Error`] if any prior write (including [`field`](Self::field)) failed.
    pub fn finish(&mut self) -> fmt::Result {
        self.result.and_then(|()| self.buf.write_char('}'))
    }
}

/// Builder for writing Haskell constructor applications: `(Constructor arg1 arg2)`.
///
/// Created by [`app`]. Bare constructor (no parens) when there are no args.
pub struct HaskellApp<'a, W: fmt::Write> {
    buf: &'a mut W,
    constructor: &'a str,
    result: fmt::Result,
    has_args: bool,
}

/// Start writing a Haskell constructor application.
///
/// ```
/// use ghci::haskell;
/// use ghci::ToHaskell;
///
/// let mut buf = String::new();
/// haskell::app(&mut buf, "Pair")
///     .arg(&1u32)
///     .arg(&true)
///     .finish()
///     .unwrap();
/// assert_eq!(buf, "(Pair 1 True)");
/// ```
pub const fn app<'a, W: fmt::Write>(buf: &'a mut W, constructor: &'a str) -> HaskellApp<'a, W> {
    HaskellApp {
        buf,
        constructor,
        result: Ok(()),
        has_args: false,
    }
}

impl<W: fmt::Write> HaskellApp<'_, W> {
    /// Write an argument. Each call may use a different `ToHaskell` type.
    pub fn arg<T: ToHaskell>(&mut self, value: &T) -> &mut Self {
        self.result = self.result.and_then(|()| {
            if !self.has_args {
                self.buf.write_char('(')?;
                self.buf.write_str(self.constructor)?;
            }
            self.buf.write_char(' ')?;
            value.write_haskell(self.buf)
        });
        self.has_args = true;
        self
    }

    /// Finish the application. Writes bare constructor if no args, closing `)` otherwise.
    ///
    /// # Errors
    ///
    /// Returns [`fmt::Error`] if any prior write failed.
    pub fn finish(&mut self) -> fmt::Result {
        self.result.and_then(|()| {
            if self.has_args {
                self.buf.write_char(')')
            } else {
                self.buf.write_str(self.constructor)
            }
        })
    }
}

// ── FromHaskell helpers ──────────────────────────────────────────────

/// Parsed record fields, returned by [`parse_record`].
///
/// Use [`field`](RecordFields::field) to extract typed values by name.
pub struct RecordFields<'a> {
    fields: Vec<(&'a str, &'a str)>,
}

/// Parse a Haskell record expression, checking the constructor name.
///
/// Returns the parsed fields and remaining input.
///
/// ```
/// use ghci::haskell;
/// use ghci::FromHaskell;
///
/// let (rec, rest) = haskell::parse_record("Point", "Point {x = 1, y = 2}").unwrap();
/// let x: u32 = rec.field("x").unwrap();
/// let y: u32 = rec.field("y").unwrap();
/// assert_eq!((x, y), (1, 2));
/// assert_eq!(rest, "");
/// ```
///
/// # Errors
///
/// Returns [`HaskellParseError`] if the constructor doesn't match or the record syntax is malformed.
pub fn parse_record<'a>(
    constructor: &str,
    input: &'a str,
) -> Result<(RecordFields<'a>, &'a str), HaskellParseError> {
    let input = skip_ws(input);
    let (name, rest) = parse_constructor(input)?;
    if name != constructor {
        return Err(HaskellParseError::ParseError {
            message: format!("expected constructor {constructor:?}, got {name:?}"),
        });
    }
    let (fields, rest) = parse_record_fields(rest)?;
    Ok((RecordFields { fields }, rest))
}

impl RecordFields<'_> {
    /// Look up a field by name and parse its value.
    ///
    /// # Errors
    ///
    /// Returns [`HaskellParseError`] if the field is not found or its value cannot be parsed.
    pub fn field<T: FromHaskell>(&self, name: &str) -> Result<T, HaskellParseError> {
        let raw = self
            .fields
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| *v)
            .ok_or_else(|| HaskellParseError::ParseError {
                message: format!("field {name:?} not found"),
            })?;
        T::from_haskell(raw)
    }
}

/// Streaming parser for Haskell constructor applications, returned by [`parse_app`].
///
/// Use [`arg`](AppParser::arg) to parse each positional argument in order,
/// then [`finish`](AppParser::finish) to consume the closing `)`.
pub struct AppParser<'a> {
    rest: &'a str,
    in_parens: bool,
}

/// Parse a Haskell constructor application, checking the constructor name.
///
/// Handles both `(Constructor arg1 arg2)` (parenthesized) and bare `Constructor`.
///
/// ```
/// use ghci::haskell;
/// use ghci::FromHaskell;
///
/// let mut p = haskell::parse_app("Pair", "(Pair 1 True)").unwrap();
/// let x: u32 = p.arg().unwrap();
/// let b: bool = p.arg().unwrap();
/// let rest = p.finish().unwrap();
/// assert_eq!((x, b), (1, true));
/// assert_eq!(rest, "");
/// ```
///
/// # Errors
///
/// Returns [`HaskellParseError`] if the constructor doesn't match.
pub fn parse_app<'a>(
    constructor: &str,
    input: &'a str,
) -> Result<AppParser<'a>, HaskellParseError> {
    let input = skip_ws(input);
    // Try parenthesized: (Constructor ...)
    if let Some(inner) = input.strip_prefix('(') {
        let inner = skip_ws(inner);
        let (name, rest) = parse_identifier(inner)?;
        if name != constructor {
            return Err(HaskellParseError::ParseError {
                message: format!("expected constructor {constructor:?}, got {name:?}"),
            });
        }
        return Ok(AppParser {
            rest,
            in_parens: true,
        });
    }
    // Bare constructor
    let (name, rest) = parse_identifier(input)?;
    if name != constructor {
        return Err(HaskellParseError::ParseError {
            message: format!("expected constructor {constructor:?}, got {name:?}"),
        });
    }
    Ok(AppParser {
        rest,
        in_parens: false,
    })
}

impl<'a> AppParser<'a> {
    /// Parse the next positional argument.
    ///
    /// # Errors
    ///
    /// Returns [`HaskellParseError`] if the argument cannot be parsed.
    pub fn arg<T: FromHaskell>(&mut self) -> Result<T, HaskellParseError> {
        let (val, rest) = T::parse_haskell(self.rest)?;
        self.rest = rest;
        Ok(val)
    }

    /// Finish parsing, consuming the closing `)` if the application was parenthesized.
    ///
    /// # Errors
    ///
    /// Returns [`HaskellParseError`] if a closing `)` is expected but not found.
    pub fn finish(self) -> Result<&'a str, HaskellParseError> {
        let rest = skip_ws(self.rest);
        if self.in_parens {
            rest.strip_prefix(')')
                .ok_or_else(|| HaskellParseError::ParseError {
                    message: format!(
                        "expected closing ')' for constructor application, got {rest:?}"
                    ),
                })
        } else {
            Ok(rest)
        }
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
        assert_eq!(x.to_haskell(), "42");
    }

    // ── FromHaskell tests ────────────────────────────────────────────

    #[test]
    fn bool_from_haskell() {
        assert!(bool::from_haskell("True").unwrap());
        assert!(!bool::from_haskell("False").unwrap());
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
        assert!(
            f64::from_haskell("(1/0)").unwrap().is_infinite()
                && f64::from_haskell("(1/0)").unwrap().is_sign_positive()
        );
        assert!(
            f64::from_haskell("((-1)/0)").unwrap().is_infinite()
                && f64::from_haskell("((-1)/0)").unwrap().is_sign_negative()
        );
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

    // ── Builder tests ──────────────────────────────────────────────────

    #[test]
    fn record_builder_single_field() {
        let mut buf = String::new();
        record(&mut buf, "Foo")
            .field("bar", &42u32)
            .finish()
            .unwrap();
        assert_eq!(buf, "Foo {bar = 42}");
    }

    #[test]
    fn record_builder_mixed_types() {
        let mut buf = String::new();
        record(&mut buf, "Point")
            .field("x", &1u32)
            .field("y", &2.5f64)
            .field("label", &"origin")
            .finish()
            .unwrap();
        assert_eq!(buf, r#"Point {x = 1, y = 2.5, label = "origin"}"#);
    }

    #[test]
    fn record_builder_no_fields() {
        let mut buf = String::new();
        record(&mut buf, "Empty").finish().unwrap();
        assert_eq!(buf, "Empty {}");
    }

    #[test]
    fn app_builder_no_args() {
        let mut buf = String::new();
        app(&mut buf, "Nothing").finish().unwrap();
        assert_eq!(buf, "Nothing");
    }

    #[test]
    fn app_builder_single_arg() {
        let mut buf = String::new();
        app(&mut buf, "Just").arg(&42u32).finish().unwrap();
        assert_eq!(buf, "(Just 42)");
    }

    #[test]
    fn app_builder_mixed_types() {
        let mut buf = String::new();
        app(&mut buf, "Pair")
            .arg(&1u32)
            .arg(&true)
            .finish()
            .unwrap();
        assert_eq!(buf, "(Pair 1 True)");
    }

    #[test]
    fn parse_record_basic() {
        let (rec, rest) = parse_record("Foo", "Foo {bar = 42, baz = True}").unwrap();
        assert_eq!(rec.field::<u32>("bar").unwrap(), 42);
        assert!(rec.field::<bool>("baz").unwrap());
        assert_eq!(rest, "");
    }

    #[test]
    fn parse_record_nested() {
        let (rec, rest) = parse_record("X", r#"X {a = (Just [1, 2]), b = "hello"}"#).unwrap();
        assert_eq!(
            rec.field::<Option<Vec<u32>>>("a").unwrap(),
            Some(vec![1, 2])
        );
        assert_eq!(rec.field::<String>("b").unwrap(), "hello");
        assert_eq!(rest, "");
    }

    #[test]
    fn parse_record_missing_field() {
        let (rec, _) = parse_record("Foo", "Foo {bar = 42}").unwrap();
        assert!(rec.field::<u32>("missing").is_err());
    }

    #[test]
    fn parse_record_wrong_constructor() {
        assert!(parse_record("Bar", "Foo {x = 1}").is_err());
    }

    #[test]
    fn parse_app_no_args() {
        let p = parse_app("Nothing", "Nothing").unwrap();
        let rest = p.finish().unwrap();
        assert_eq!(rest, "");
    }

    #[test]
    fn parse_app_single_arg() {
        let mut p = parse_app("Just", "(Just 42)").unwrap();
        let val: u32 = p.arg().unwrap();
        assert_eq!(val, 42);
        let rest = p.finish().unwrap();
        assert_eq!(rest, "");
    }

    #[test]
    fn parse_app_mixed_types() {
        let mut p = parse_app("Pair", "(Pair 1 True)").unwrap();
        let x: u32 = p.arg().unwrap();
        let b: bool = p.arg().unwrap();
        assert_eq!((x, b), (1, true));
        let rest = p.finish().unwrap();
        assert_eq!(rest, "");
    }

    #[test]
    fn parse_app_wrong_constructor() {
        assert!(parse_app("Bar", "(Foo 1)").is_err());
    }

    #[test]
    fn record_roundtrip() {
        let mut buf = String::new();
        record(&mut buf, "Point")
            .field("x", &10i32)
            .field("y", &(-3i32))
            .field("name", &"test")
            .finish()
            .unwrap();

        let (rec, rest) = parse_record("Point", &buf).unwrap();
        assert_eq!(rec.field::<i32>("x").unwrap(), 10);
        assert_eq!(rec.field::<i32>("y").unwrap(), -3);
        assert_eq!(rec.field::<String>("name").unwrap(), "test");
        assert_eq!(rest, "");
    }

    #[test]
    fn app_roundtrip() {
        let mut buf = String::new();
        app(&mut buf, "Pair")
            .arg(&42u32)
            .arg(&"hello".to_string())
            .finish()
            .unwrap();

        let mut p = parse_app("Pair", &buf).unwrap();
        assert_eq!(p.arg::<u32>().unwrap(), 42);
        assert_eq!(p.arg::<String>().unwrap(), "hello");
        assert_eq!(p.finish().unwrap(), "");
    }
}
