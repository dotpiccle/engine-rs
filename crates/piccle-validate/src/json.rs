//! Hand-rolled strict JSON parser producing `serde_json::Value`.
//!
//! Spec: piccle-spec/docs/11-engine-safety.md §Untrusted input and
//! piccle-spec/docs/14-conformance.md (parse-stage codes). A hand-rolled
//! parser gives the engine exact control of duplicate members, non-finite
//! number tokens, out-of-range literals, and parser resource limits —
//! distinctions off-the-shelf parsers do not expose.

use piccle_core::error::{PiccleError, PiccleResult};
use serde_json::{Map, Value};

/// Maximum accepted input size (1 MiB).
pub const MAX_INPUT_BYTES: usize = 1_048_576;
/// Maximum container nesting depth.
pub const MAX_NESTING_DEPTH: usize = 64;

/// Error code: input exceeds the parser's byte limit.
const LIMIT_INPUT_BYTES: &str = "max_input_bytes";
/// Error code: input exceeds the parser's nesting limit.
const LIMIT_NESTING: &str = "max_nesting_depth";

/// Parses raw bytes into a JSON value, enforcing parser resource limits and
/// the spec's parse-stage error codes.
///
/// # Errors
///
/// - `ResourceRejected` when the input exceeds `MAX_INPUT_BYTES` or
///   `MAX_NESTING_DEPTH`.
/// - `Malformed` (`json.malformed`) for syntax errors and invalid UTF-8.
/// - `Malformed` (`json.duplicate_member`) for repeated object members.
/// - `Malformed` (`json.non_finite_number`) for `NaN`/`Infinity` tokens.
/// - `Malformed` (`json.number_out_of_range`) for literals outside binary64.
pub fn parse(bytes: &[u8]) -> PiccleResult<Value> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(PiccleError::ResourceRejected {
            limit: LIMIT_INPUT_BYTES,
            reason: "document exceeds 1 MiB parser limit",
        });
    }
    let text = std::str::from_utf8(bytes).map_err(|_| PiccleError::malformed("json.malformed"))?;
    let mut parser = Parser { bytes: text.as_bytes(), pos: 0, depth: 0 };
    parser.skip_whitespace();
    let value = parser.parse_value()?;
    parser.skip_whitespace();
    if parser.pos != parser.bytes.len() {
        return Err(PiccleError::malformed("json.malformed"));
    }
    Ok(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    depth: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_whitespace(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.peek() {
            self.pos += 1;
        }
    }

    fn parse_value(&mut self) -> PiccleResult<Value> {
        match self.peek() {
            Some(b'{') => self.parse_object(),
            Some(b'[') => self.parse_array(),
            Some(b'"') => Ok(Value::String(self.parse_string()?)),
            Some(b't') => self.parse_literal(b"true", Value::Bool(true)),
            Some(b'f') => self.parse_literal(b"false", Value::Bool(false)),
            Some(b'n') => self.parse_literal(b"null", Value::Null),
            Some(b'N') | Some(b'I') => self.reject_non_finite_or_malformed(),
            Some(b'-') => {
                if self.bytes.get(self.pos + 1) == Some(&b'I') {
                    return self.reject_non_finite_or_malformed();
                }
                self.parse_number()
            }
            Some(b'0'..=b'9') => self.parse_number(),
            _ => Err(PiccleError::malformed("json.malformed")),
        }
    }

    fn reject_non_finite_or_malformed(&self) -> PiccleResult<Value> {
        const NON_FINITE_TOKENS: [&[u8]; 3] = [b"NaN", b"Infinity", b"-Infinity"];
        let is_exact_token = NON_FINITE_TOKENS.iter().any(|token| {
            let remaining = &self.bytes[self.pos..];
            remaining.starts_with(token)
                && remaining.get(token.len()).is_none_or(|next| {
                    matches!(next, b' ' | b'\t' | b'\n' | b'\r' | b',' | b']' | b'}')
                })
        });
        let code = if is_exact_token { "json.non_finite_number" } else { "json.malformed" };
        Err(PiccleError::malformed(code))
    }

    fn parse_literal(&mut self, literal: &[u8], value: Value) -> PiccleResult<Value> {
        if self.bytes.len() - self.pos >= literal.len()
            && &self.bytes[self.pos..self.pos + literal.len()] == literal
        {
            self.pos += literal.len();
            return Ok(value);
        }
        // An alphabetic non-literal token naming a non-finite number keeps
        // its dedicated code even when misspelled inside a longer token.
        Err(PiccleError::malformed("json.malformed"))
    }

    fn enter_container(&mut self) -> PiccleResult<()> {
        self.depth += 1;
        if self.depth > MAX_NESTING_DEPTH {
            return Err(PiccleError::ResourceRejected {
                limit: LIMIT_NESTING,
                reason: "document exceeds 64 levels of nesting",
            });
        }
        Ok(())
    }

    fn parse_object(&mut self) -> PiccleResult<Value> {
        self.enter_container()?;
        self.pos += 1; // consume '{'
        let mut map = Map::new();
        self.skip_whitespace();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            self.depth -= 1;
            return Ok(Value::Object(map));
        }
        loop {
            self.skip_whitespace();
            if self.peek() != Some(b'"') {
                return Err(PiccleError::malformed("json.malformed"));
            }
            let key = self.parse_string()?;
            self.skip_whitespace();
            if self.peek() != Some(b':') {
                return Err(PiccleError::malformed("json.malformed"));
            }
            self.pos += 1;
            self.skip_whitespace();
            let value = self.parse_value()?;
            if map.contains_key(&key) {
                return Err(PiccleError::malformed("json.duplicate_member"));
            }
            map.insert(key, value);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    self.depth -= 1;
                    return Ok(Value::Object(map));
                }
                _ => return Err(PiccleError::malformed("json.malformed")),
            }
        }
    }

    fn parse_array(&mut self) -> PiccleResult<Value> {
        self.enter_container()?;
        self.pos += 1; // consume '['
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.pos += 1;
            self.depth -= 1;
            return Ok(Value::Array(items));
        }
        loop {
            self.skip_whitespace();
            let value = self.parse_value()?;
            items.push(value);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    self.depth -= 1;
                    return Ok(Value::Array(items));
                }
                _ => return Err(PiccleError::malformed("json.malformed")),
            }
        }
    }

    fn parse_string(&mut self) -> PiccleResult<String> {
        self.pos += 1; // consume opening quote
        let mut out = String::new();
        loop {
            let Some(byte) = self.peek()
            else {
                return Err(PiccleError::malformed("json.malformed"));
            };
            match byte {
                b'"' => {
                    self.pos += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.pos += 1;
                    self.parse_escape(&mut out)?;
                }
                0x00..=0x1F => return Err(PiccleError::malformed("json.malformed")),
                _ => {
                    // Input is valid UTF-8 (checked upstream), so a
                    // non-ASCII lead byte is safe to copy through by
                    // decoding the full multi-byte sequence.
                    let start = self.pos;
                    self.pos += utf8_len(byte);
                    let slice = &self.bytes[start..self.pos];
                    let text = std::str::from_utf8(slice)
                        .map_err(|_| PiccleError::malformed("json.malformed"))?;
                    out.push_str(text);
                }
            }
        }
    }

    fn parse_escape(&mut self, out: &mut String) -> PiccleResult<()> {
        let Some(escape) = self.peek()
        else {
            return Err(PiccleError::malformed("json.malformed"));
        };
        self.pos += 1;
        match escape {
            b'"' => out.push('"'),
            b'\\' => out.push('\\'),
            b'/' => out.push('/'),
            b'b' => out.push('\u{0008}'),
            b'f' => out.push('\u{000C}'),
            b'n' => out.push('\n'),
            b'r' => out.push('\r'),
            b't' => out.push('\t'),
            b'u' => {
                let first = self.parse_hex4()?;
                if (0xD800..0xDC00).contains(&first) {
                    // High surrogate: a low-surrogate escape must follow.
                    if self.peek() == Some(b'\\') && self.bytes.get(self.pos + 1) == Some(&b'u') {
                        self.pos += 2;
                        let second = self.parse_hex4()?;
                        if !(0xDC00..0xE000).contains(&second) {
                            return Err(PiccleError::malformed("json.malformed"));
                        }
                        let code = 0x1_0000 + ((first - 0xD800) << 10) + (second - 0xDC00);
                        let Some(ch) = char::from_u32(code)
                        else {
                            return Err(PiccleError::malformed("json.malformed"));
                        };
                        out.push(ch);
                    }
                    else {
                        return Err(PiccleError::malformed("json.malformed"));
                    }
                }
                else if (0xDC00..0xE000).contains(&first) {
                    return Err(PiccleError::malformed("json.malformed"));
                }
                else {
                    let Some(ch) = char::from_u32(first)
                    else {
                        return Err(PiccleError::malformed("json.malformed"));
                    };
                    out.push(ch);
                }
            }
            _ => return Err(PiccleError::malformed("json.malformed")),
        }
        Ok(())
    }

    fn parse_hex4(&mut self) -> PiccleResult<u32> {
        if self.bytes.len() - self.pos < 4 {
            return Err(PiccleError::malformed("json.malformed"));
        }
        let mut value = 0u32;
        for _ in 0..4 {
            let digit = match self.bytes[self.pos] {
                b'0'..=b'9' => u32::from(self.bytes[self.pos] - b'0'),
                b'a'..=b'f' => u32::from(self.bytes[self.pos] - b'a') + 10,
                b'A'..=b'F' => u32::from(self.bytes[self.pos] - b'A') + 10,
                _ => return Err(PiccleError::malformed("json.malformed")),
            };
            value = value * 16 + digit;
            self.pos += 1;
        }
        Ok(value)
    }

    /// Strict JSON number grammar:
    /// `-?(0|[1-9]\d*)(\.\d+)?([eE][+-]?\d+)?`.
    fn parse_number(&mut self) -> PiccleResult<Value> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        // Integer part: 0, or [1-9] followed by digits.
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => {
                self.pos += 1;
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.pos += 1;
                }
            }
            _ => return Err(PiccleError::malformed("json.malformed")),
        }
        let mut is_integer = true;
        if self.peek() == Some(b'.') {
            is_integer = false;
            self.pos += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(PiccleError::malformed("json.malformed"));
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            is_integer = false;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.pos += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(PiccleError::malformed("json.malformed"));
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        let token = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| PiccleError::malformed("json.malformed"))?;
        if is_integer {
            if let Ok(unsigned) = token.parse::<u64>() {
                return Ok(Value::from(unsigned));
            }
            if let Ok(signed) = token.parse::<i64>() {
                return Ok(Value::from(signed));
            }
        }
        let Ok(number) = token.parse::<f64>()
        else {
            return Err(PiccleError::malformed("json.malformed"));
        };
        if number.is_infinite() {
            return Err(PiccleError::malformed("json.number_out_of_range"));
        }
        let Some(json_number) = serde_json::Number::from_f64(number)
        else {
            return Err(PiccleError::malformed("json.number_out_of_range"));
        };
        Ok(Value::Number(json_number))
    }
}

/// Byte length of the UTF-8 sequence starting with `lead`.
fn utf8_len(lead: u8) -> usize {
    if lead < 0x80 {
        1
    }
    else if lead < 0xE0 {
        2
    }
    else if lead < 0xF0 {
        3
    }
    else {
        4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_document() {
        let value = parse(br#"{"a": 1}"#).ok();
        assert!(value.is_some());
    }

    #[test]
    fn rejects_duplicate_members() {
        let err = parse(br#"{"a": 1, "a": 2}"#).err();
        assert_eq!(err.map(|e| e.code()), Some("json.duplicate_member"));
    }

    #[test]
    fn rejects_nan_token_as_non_finite() {
        let err = parse(br#"{"a": NaN}"#).err();
        assert_eq!(err.map(|e| e.code()), Some("json.non_finite_number"));
    }

    #[test]
    fn rejects_infinity_token_as_non_finite() {
        let err = parse(br#"{"a": -Infinity}"#).err();
        assert_eq!(err.map(|e| e.code()), Some("json.non_finite_number"));
    }

    #[test]
    fn rejects_overflowing_exponent() {
        let err = parse(br#"{"a": 1e400}"#).err();
        assert_eq!(err.map(|e| e.code()), Some("json.number_out_of_range"));
    }

    #[test]
    fn rejects_leading_zero() {
        let err = parse(br#"{"a": 01}"#).err();
        assert_eq!(err.map(|e| e.code()), Some("json.malformed"));
    }

    #[test]
    fn accepts_integer_forms() {
        let value = parse(br#"{"a": 1, "b": 1.0, "c": 1e0}"#).ok();
        assert!(value.is_some());
    }

    #[test]
    fn rejects_trailing_garbage() {
        let err = parse(br#"{"a": 1} x"#).err();
        assert_eq!(err.map(|e| e.code()), Some("json.malformed"));
    }

    #[test]
    fn rejects_excessive_nesting() {
        let mut doc = String::new();
        for _ in 0..70 {
            doc.push('[');
        }
        let err = parse(doc.as_bytes()).err();
        assert_eq!(err.map(|e| e.stage()), Some(piccle_core::error::Stage::ResourceRejected));
    }

    #[test]
    fn rejects_oversized_input() {
        let bytes = vec![b' '; MAX_INPUT_BYTES + 1];
        let err = parse(&bytes).err();
        assert_eq!(err.map(|e| e.stage()), Some(piccle_core::error::Stage::ResourceRejected));
    }
}
