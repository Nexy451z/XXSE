//! A small, strict JSON reader.
//!
//! The updater's only input from the network is GitHub's release document, and
//! this crate is no_std + alloc, so there is no serde and nothing is allowed to
//! pull a tree-walking crate in. This is the reader that document needs - and
//! nothing more: it accepts exactly RFC 8259 JSON, rejects the rest with the
//! byte offset it stopped on, and never recurses deeper than MAX_DEPTH so a
//! hostile answer cannot walk the console's stack off the end.
//!
//! Numbers keep their source text instead of going through f64: the fields the
//! updater reads (size, id) are integers larger than 2^53 is safe for, and core
//! has no float parsing without libm.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Nesting limit. A release document is three levels deep; the limit is here so
/// that a malformed or malicious answer fails instead of overflowing the stack.
pub const MAX_DEPTH: usize = 64;

/// A number, exactly as it was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Num {
    text: String,
}

impl Num {
    /// The value as an unsigned integer, if it is one.
    pub fn as_u64(&self) -> Option<u64> {
        if self.text.is_empty() || !self.text.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        // Accumulate by hand: the explicit loop makes the overflow answer
        // obvious (None, not a panic).
        let mut value: u64 = 0;
        for b in self.text.bytes() {
            value = value.checked_mul(10)?.checked_add((b - b'0') as u64)?;
        }
        Some(value)
    }

    /// The literal as written, e.g. "-0.25" or "6".
    pub fn as_str(&self) -> &str {
        &self.text
    }
}

/// One JSON value.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(Num),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    /// The member of an object, if this is an object that has it.
    ///
    /// Duplicate keys resolve to the last one, which is what every browser does.
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(members) => members.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Json::Num(n) => n.as_u64(),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(items) => Some(items),
            _ => None,
        }
    }

    /// The text of a string member, or the empty string when it is missing.
    pub fn str_at(&self, key: &str) -> &str {
        self.get(key).and_then(Json::as_str).unwrap_or("")
    }

    /// The unsigned integer of a member, or 0 when it is missing.
    pub fn u64_at(&self, key: &str) -> u64 {
        self.get(key).and_then(Json::as_u64).unwrap_or(0)
    }
}

/// Why a document was refused. offset is a byte offset into the input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonError {
    pub offset: usize,
    pub message: &'static str,
}

impl core::fmt::Display for JsonError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "JSON を読めません（{} バイト目: {}）", self.offset, self.message)
    }
}

type PResult<T> = core::result::Result<T, JsonError>;

struct Parser<'a> {
    src: &'a [u8],
    pos: usize,
    depth: usize,
}

/// Parse one complete JSON document. Trailing non-whitespace is an error.
pub fn parse(input: &str) -> PResult<Json> {
    let mut p = Parser { src: input.as_bytes(), pos: 0, depth: 0 };
    p.skip_ws();
    let value = p.value()?;
    p.skip_ws();
    if p.pos != p.src.len() {
        return Err(p.err("余分な文字があります"));
    }
    Ok(value)
}

impl<'a> Parser<'a> {
    fn err(&self, message: &'static str) -> JsonError {
        JsonError { offset: self.pos, message }
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            match b {
                b' ' | b'\t' | b'\n' | b'\r' => self.pos += 1,
                _ => break,
            }
        }
    }

    fn eat(&mut self, b: u8) -> bool {
        if self.peek() == Some(b) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect_lit(&mut self, lit: &'static str) -> PResult<()> {
        let bytes = lit.as_bytes();
        if self.src.len() < self.pos + bytes.len()
            || &self.src[self.pos..self.pos + bytes.len()] != bytes
        {
            return Err(self.err("リテラルが違います"));
        }
        self.pos += bytes.len();
        Ok(())
    }

    fn value(&mut self) -> PResult<Json> {
        if self.depth > MAX_DEPTH {
            return Err(self.err("入れ子が深すぎます"));
        }
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') => {
                self.expect_lit("true")?;
                Ok(Json::Bool(true))
            }
            Some(b'f') => {
                self.expect_lit("false")?;
                Ok(Json::Bool(false))
            }
            Some(b'n') => {
                self.expect_lit("null")?;
                Ok(Json::Null)
            }
            Some(b'-') | Some(b'0'..=b'9') => self.number(),
            Some(_) => Err(self.err("値がありません")),
            None => Err(self.err("途中で終わっています")),
        }
    }

    fn object(&mut self) -> PResult<Json> {
        self.pos += 1; // '{'
        self.depth += 1;
        let mut members: Vec<(String, Json)> = Vec::new();
        self.skip_ws();
        if self.eat(b'}') {
            self.depth -= 1;
            return Ok(Json::Obj(members));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            if !self.eat(b':') {
                return Err(self.err("':' がありません"));
            }
            self.skip_ws();
            let value = self.value()?;
            members.push((key, value));
            self.skip_ws();
            if self.eat(b',') {
                continue;
            }
            if self.eat(b'}') {
                self.depth -= 1;
                return Ok(Json::Obj(members));
            }
            return Err(self.err("'}' がありません"));
        }
    }

    fn array(&mut self) -> PResult<Json> {
        self.pos += 1; // '['
        self.depth += 1;
        let mut items: Vec<Json> = Vec::new();
        self.skip_ws();
        if self.eat(b']') {
            self.depth -= 1;
            return Ok(Json::Arr(items));
        }
        loop {
            self.skip_ws();
            items.push(self.value()?);
            self.skip_ws();
            if self.eat(b',') {
                continue;
            }
            if self.eat(b']') {
                self.depth -= 1;
                return Ok(Json::Arr(items));
            }
            return Err(self.err("']' がありません"));
        }
    }

    fn hex4(&mut self) -> PResult<u32> {
        let mut value: u32 = 0;
        for _ in 0..4 {
            let b = self.peek().ok_or_else(|| self.err("16進エスケープが途中で終わっています"))?;
            let digit = match b {
                b'0'..=b'9' => (b - b'0') as u32,
                b'a'..=b'f' => (b - b'a') as u32 + 10,
                b'A'..=b'F' => (b - b'A') as u32 + 10,
                _ => return Err(self.err("16進エスケープが違います")),
            };
            value = value * 16 + digit;
            self.pos += 1;
        }
        Ok(value)
    }

    fn string(&mut self) -> PResult<String> {
        if !self.eat(b'"') {
            return Err(self.err("文字列の始まりがありません"));
        }
        let mut out = String::new();
        loop {
            let b = self.peek().ok_or_else(|| self.err("文字列が終わりません"))?;
            match b {
                b'"' => {
                    self.pos += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.pos += 1;
                    let esc = self.peek().ok_or_else(|| self.err("エスケープが途中です"))?;
                    self.pos += 1;
                    match esc {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hi = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&hi) {
                                // A high surrogate must be followed by a low one.
                                if !(self.eat(b'\\') && self.eat(b'u')) {
                                    return Err(self.err("サロゲートペアが欠けています"));
                                }
                                let lo = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&lo) {
                                    return Err(self.err("サロゲートペアが違います"));
                                }
                                0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                            } else if (0xDC00..0xE000).contains(&hi) {
                                return Err(self.err("単独のサロゲートです"));
                            } else {
                                hi
                            };
                            match char::from_u32(code) {
                                Some(c) => out.push(c),
                                None => return Err(self.err("文字コードが違います")),
                            }
                        }
                        _ => return Err(self.err("知らないエスケープです")),
                    }
                }
                // Control characters may not appear raw inside a string.
                0x00..=0x1F => return Err(self.err("制御文字が文字列に入っています")),
                _ => {
                    // Copy the whole UTF-8 sequence: the input came in as &str,
                    // so its bytes are already valid.
                    let len = utf8_len(b);
                    let end = self.pos + len;
                    if end > self.src.len() {
                        return Err(self.err("文字列が終わりません"));
                    }
                    let piece = core::str::from_utf8(&self.src[self.pos..end])
                        .map_err(|_| self.err("UTF-8 が違います"))?;
                    out.push_str(piece);
                    self.pos = end;
                }
            }
        }
    }

    fn number(&mut self) -> PResult<Json> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        // Integer part: "0" or [1-9][0-9]*
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => {
                while let Some(b'0'..=b'9') = self.peek() {
                    self.pos += 1;
                }
            }
            _ => return Err(self.err("数値がありません")),
        }
        // Fraction
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.err("小数部がありません"));
            }
            while let Some(b'0'..=b'9') = self.peek() {
                self.pos += 1;
            }
        }
        // Exponent
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.pos += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.err("指数部がありません"));
            }
            while let Some(b'0'..=b'9') = self.peek() {
                self.pos += 1;
            }
        }
        let text = core::str::from_utf8(&self.src[start..self.pos])
            .map_err(|_| self.err("数値が読めません"))?
            .to_string();
        Ok(Json::Num(Num { text }))
    }
}

fn utf8_len(lead: u8) -> usize {
    match lead {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}
