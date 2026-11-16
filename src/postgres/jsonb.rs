//! PostgreSQL 18's `jsonb`, read as `jsonb_in` reads it, printed as `jsonb_out` prints it, and
//! equal, ordered and hashed as `compareJsonbContainers` compares it for the btree order.
//!
//! A value is held as the token stream `JsonbIteratorNext` walks, so parsing, printing,
//! comparing and dropping never recurse, and nesting of any depth stays within the stack.

use alloc::{string::String, vec::Vec};
use core::{
    cmp::Ordering,
    fmt::{self, Write},
    hash::{Hash, Hasher},
    mem,
};

use super::numeric::{NumericError, PgNumeric};

/// Why `jsonb_in` refuses a text, one variant per detail PostgreSQL gives, each printed as
/// PostgreSQL's message and detail without the token it quotes.
///
/// ```
/// use sql_values::postgres::{JsonbError, NumericError, PgJsonb};
///
/// assert_eq!(PgJsonb::parse(r#"{"a":1,}"#), Err(JsonbError::ExpectedString));
/// assert_eq!(PgJsonb::parse("01"), Err(JsonbError::InvalidToken));
/// assert_eq!(PgJsonb::parse(r#""\ud83d""#), Err(JsonbError::LowSurrogateExpected));
/// assert_eq!(
///     PgJsonb::parse("1e1000000"),
///     Err(JsonbError::Numeric(NumericError::FormatOverflow)),
/// );
/// assert_eq!(
///     PgJsonb::parse(r#""\u0000""#).unwrap_err().to_string(),
///     r"unsupported Unicode escape sequence: \u0000 cannot be converted to text",
/// );
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum JsonbError {
    /// A word other than `true`, `false` and `null`, a malformed number, a character that
    /// starts no token, or a string the input ends inside.
    #[error("invalid input syntax for type json: Token is invalid")]
    InvalidToken,
    /// A backslash followed by a character that escapes nothing.
    #[error("invalid input syntax for type json: Escape sequence is invalid")]
    InvalidEscape,
    /// A control character below `0x20` written in a string without an escape.
    #[error("invalid input syntax for type json: Character must be escaped")]
    UnescapedControlCharacter,
    /// A `\u` not followed by four hexadecimal digits.
    #[error(
        "invalid input syntax for type json: \"\\u\" must be followed by four hexadecimal digits"
    )]
    UnicodeEscapeFormat,
    /// A high surrogate escape directly after another.
    #[error(
        "invalid input syntax for type json: Unicode high surrogate must not follow a high surrogate"
    )]
    HighSurrogateRepeated,
    /// A low surrogate escape without a high surrogate before it, or a high surrogate escape
    /// followed by anything but a low surrogate escape.
    #[error(
        "invalid input syntax for type json: Unicode low surrogate must follow a high surrogate"
    )]
    LowSurrogateExpected,
    /// The escape `\u0000`, which `text` cannot hold.
    #[error("unsupported Unicode escape sequence: \\u0000 cannot be converted to text")]
    CodePointZero,
    /// A token where a value belongs.
    #[error("invalid input syntax for type json: Expected JSON value")]
    ExpectedValue,
    /// A token other than a key after a comma in an object.
    #[error("invalid input syntax for type json: Expected string")]
    ExpectedString,
    /// A token other than a key or `}` after `{`.
    #[error("invalid input syntax for type json: Expected string or \"}}\"")]
    ExpectedStringOrObjectEnd,
    /// A token other than `:` after a key.
    #[error("invalid input syntax for type json: Expected \":\"")]
    ExpectedColon,
    /// A token other than `,` or `}` after a value in an object.
    #[error("invalid input syntax for type json: Expected \",\" or \"}}\"")]
    ExpectedCommaOrObjectEnd,
    /// A token other than `,` or `]` after an element in an array.
    #[error("invalid input syntax for type json: Expected \",\" or \"]\"")]
    ExpectedCommaOrArrayEnd,
    /// A token after the whole value.
    #[error("invalid input syntax for type json: Expected end of input")]
    ExpectedEnd,
    /// Input that ends before its value does.
    #[error("invalid input syntax for type json: The input string ended unexpectedly")]
    UnexpectedEnd,
    /// A number `numeric_in` refuses, one beyond the weight or scale PostgreSQL stores.
    #[error(transparent)]
    Numeric(NumericError),
}

/// The kind of a top-level `jsonb` value, as `cannotCastJsonbValue` names it when a cast to a
/// scalar type refuses it.
///
/// ```
/// use sql_values::postgres::JsonbKind;
///
/// assert_eq!(JsonbKind::Numeric.to_string(), "numeric");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JsonbKind {
    /// `null`.
    Null,
    /// A string.
    String,
    /// A number.
    Numeric,
    /// `true` or `false`.
    Boolean,
    /// An array.
    Array,
    /// An object.
    Object,
}

impl fmt::Display for JsonbKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Null => "null",
            Self::String => "string",
            Self::Numeric => "numeric",
            Self::Boolean => "boolean",
            Self::Array => "array",
            Self::Object => "object",
        })
    }
}

/// A PostgreSQL `jsonb` value, its object keys in `jsonb`'s storage order with the last of
/// duplicate keys kept, equal, ordered and hashed as `jsonb`'s btree operator class compares it.
///
/// The order puts an empty top-level array first, then `null`, strings, numbers, booleans,
/// arrays and objects. Arrays compare by element count and then element by element, objects by
/// pair count and then key and value in storage order, and numbers by value, so `1.0` equals
/// `1`. Strings and keys compare by their bytes, as under the `C` collation. A server whose
/// default collation is another one orders strings by it, while equality stays the same.
///
/// Its [`Display`](fmt::Display) is `jsonb_out`, so equal values can print differently.
///
/// ```
/// use sql_values::postgres::PgJsonb;
///
/// let jsonb = |text| PgJsonb::parse(text).unwrap();
/// assert_eq!(jsonb(r#"{"bb":1,"a":2,"c":3,"a":4}"#).to_string(), r#"{"a": 4, "c": 3, "bb": 1}"#);
/// assert_eq!(jsonb(" [ 1 , {\"b\" : null } ] ").to_string(), r#"[1, {"b": null}]"#);
/// assert_eq!(jsonb("1.0"), jsonb("1"));
/// assert_eq!(jsonb("1.0").to_string(), "1.0");
/// assert!(jsonb("[]") < jsonb("null"));
/// assert!(jsonb(r#""B""#) < jsonb(r#""a""#));
/// assert!(jsonb("true") < jsonb("[null]"));
/// assert!(jsonb(r#"{"b": 1}"#) < jsonb(r#"{"a": 1, "b": 2}"#));
/// ```
#[derive(Debug, Clone)]
pub struct PgJsonb {
    /// The tokens in iteration order, each container's pairs or elements between its start and
    /// end.
    tokens: Vec<Token>,
    /// The decoded text of every string and key, each token's span into it.
    strings: String,
}

/// One token of a value, as `JsonbIteratorNext` yields it.
#[derive(Debug, Clone)]
enum Token {
    Null,
    Bool(bool),
    Number(PgNumeric),
    String(Span),
    Key(Span),
    /// An array's start and its element count.
    ArrayStart(usize),
    ArrayEnd,
    /// An object's start and its pair count.
    ObjectStart(usize),
    ObjectEnd,
}

/// A byte range of [`PgJsonb::strings`] or of the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Span {
    start: usize,
    end: usize,
}

impl Token {
    /// The rank of the token's `jbvType`, the order between values of different types.
    const fn rank(&self) -> u8 {
        match self {
            Self::Null => 0,
            Self::String(_) | Self::Key(_) => 1,
            Self::Number(_) => 2,
            Self::Bool(_) => 3,
            Self::ArrayStart(_) | Self::ArrayEnd => 4,
            Self::ObjectStart(_) | Self::ObjectEnd => 5,
        }
    }
}

impl PgJsonb {
    /// PostgreSQL's `jsonb_in`, reading one JSON value between blanks, its numbers read by
    /// `numeric_in` and its `\u` escapes decoded with surrogate pairs combined.
    ///
    /// PostgreSQL also refuses nesting deeper than its `max_stack_depth` allows and strings or
    /// containers beyond the 256 MiB its binary format holds. Those limits depend on the
    /// server, and this reads any depth and size.
    ///
    /// # Errors
    ///
    /// The [`JsonbError`] PostgreSQL raises, the first one in the text as it reads it.
    ///
    /// ```
    /// use sql_values::postgres::{JsonbError, PgJsonb};
    ///
    /// let printed = |text| PgJsonb::parse(text).map(|jsonb| jsonb.to_string());
    /// assert_eq!(printed("1e2").unwrap(), "100");
    /// assert_eq!(printed("-0.0").unwrap(), "0.0");
    /// assert_eq!(printed(r#""\u00e9 \ud83d\ude00 \/""#).unwrap(), "\"é 😀 /\"");
    /// assert_eq!(printed("[1 2]"), Err(JsonbError::ExpectedCommaOrArrayEnd));
    /// assert_eq!(printed("[1,"), Err(JsonbError::UnexpectedEnd));
    /// ```
    pub fn parse(text: &str) -> Result<Self, JsonbError> {
        Parser {
            text,
            at: 0,
            tokens: Vec::new(),
            strings: String::new(),
            open: Vec::new(),
            keys: Vec::new(),
        }
        .parse()
    }

    /// The text of a string or key token's span.
    fn text(&self, span: Span) -> &str {
        &self.strings[span.start..span.end]
    }

    /// The top-level scalar, or `None` for an array or an object.
    fn scalar(&self) -> Option<&Token> {
        match self.tokens.as_slice() {
            [scalar] => Some(scalar),
            _ => None,
        }
    }

    /// The kind of the top-level value.
    fn kind(&self) -> JsonbKind {
        match self.tokens[0] {
            Token::Null => JsonbKind::Null,
            Token::String(_) | Token::Key(_) => JsonbKind::String,
            Token::Number(_) => JsonbKind::Numeric,
            Token::Bool(_) => JsonbKind::Boolean,
            Token::ArrayStart(_) | Token::ArrayEnd => JsonbKind::Array,
            Token::ObjectStart(_) | Token::ObjectEnd => JsonbKind::Object,
        }
    }

    /// The boolean `jsonb_bool` casts the value to, `None` for `null`.
    pub(super) fn to_bool(&self) -> Result<Option<bool>, JsonbKind> {
        match self.scalar() {
            Some(Token::Null) => Ok(None),
            Some(Token::Bool(value)) => Ok(Some(*value)),
            _ => Err(self.kind()),
        }
    }

    /// The number `jsonb_numeric` casts the value to, `None` for `null`.
    pub(super) fn into_numeric(self) -> Result<Option<PgNumeric>, JsonbKind> {
        let kind = self.kind();
        match <[Token; 1]>::try_from(self.tokens) {
            Ok([Token::Null]) => Ok(None),
            Ok([Token::Number(value)]) => Ok(Some(value)),
            _ => Err(kind),
        }
    }

    /// `compareJsonbScalarValue` for two scalars, or the order of their `jbvType`s, between two
    /// tokens at the same place of both streams.
    fn cmp_token(&self, left: &Token, other: &Self, right: &Token) -> Ordering {
        match (left, right) {
            (Token::Bool(left), Token::Bool(right)) => left.cmp(right),
            (Token::Number(left), Token::Number(right)) => left.cmp(right),
            (Token::String(left) | Token::Key(left), Token::String(right) | Token::Key(right)) => {
                self.text(*left).cmp(other.text(*right))
            }
            (Token::ArrayStart(left), Token::ArrayStart(right))
            | (Token::ObjectStart(left), Token::ObjectStart(right)) => left.cmp(right),
            _ => left.rank().cmp(&right.rank()),
        }
    }
}

/// The parse of one text, `jsonb_in`'s recursive descent run over explicit stacks.
struct Parser<'a> {
    text: &'a str,
    /// The byte the next token starts at or before.
    at: usize,
    tokens: Vec<Token>,
    strings: String,
    /// The nonempty containers open around the current place, innermost last.
    open: Vec<Open>,
    /// The keys of every open object, the innermost object's last.
    keys: Vec<Key>,
}

/// A nonempty container still open.
#[derive(Clone, Copy)]
struct Open {
    /// Whether the container is an object.
    object: bool,
    /// The index of its start token.
    start: usize,
    /// The length of [`Parser::keys`] when it opened.
    keys: usize,
    /// The values read in it so far.
    count: usize,
}

/// A key of an open object.
#[derive(Clone, Copy)]
struct Key {
    /// The index of its token, which starts its pair.
    token: usize,
    /// Its decoded text.
    span: Span,
}

/// A token of the JSON text, as `json_lex` reads it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Lexeme {
    ObjectStart,
    ObjectEnd,
    ArrayStart,
    ArrayEnd,
    Comma,
    Colon,
    /// A string, decoded into the parser's strings.
    String(Span),
    /// A number, its span in the text.
    Number(Span),
    True,
    False,
    Null,
    End,
}

/// `error`, or [`JsonbError::UnexpectedEnd`] when the token is the end of the input, as
/// `report_parse_error` chooses.
fn expected(lexeme: Lexeme, error: JsonbError) -> JsonbError {
    if lexeme == Lexeme::End {
        JsonbError::UnexpectedEnd
    } else {
        error
    }
}

/// `JSON_ALPHANUMERIC_CHAR`, a byte the lexer reads as part of a word.
const fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

/// `lengthCompareJsonbString`, the storage order of keys, shorter first and then by bytes.
fn storage_order(left: &str, right: &str) -> Ordering {
    left.len()
        .cmp(&right.len())
        .then_with(|| left.as_bytes().cmp(right.as_bytes()))
}

impl Parser<'_> {
    /// `pg_parse_json` with `jsonb_in`'s semantic actions.
    fn parse(mut self) -> Result<PgJsonb, JsonbError> {
        let mut lexeme = self.lex()?;
        'value: loop {
            // `lexeme` starts a value.
            match lexeme {
                Lexeme::ObjectStart => {
                    lexeme = self.lex()?;
                    match lexeme {
                        Lexeme::String(key) => {
                            self.open(true);
                            lexeme = self.key(key)?;
                            continue 'value;
                        }
                        Lexeme::ObjectEnd => {
                            lexeme = self.lex()?;
                            self.tokens
                                .extend([Token::ObjectStart(0), Token::ObjectEnd]);
                        }
                        other => {
                            return Err(expected(other, JsonbError::ExpectedStringOrObjectEnd));
                        }
                    }
                }
                Lexeme::ArrayStart => {
                    lexeme = self.lex()?;
                    if lexeme != Lexeme::ArrayEnd {
                        self.open(false);
                        continue 'value;
                    }
                    lexeme = self.lex()?;
                    self.tokens.extend([Token::ArrayStart(0), Token::ArrayEnd]);
                }
                Lexeme::String(_)
                | Lexeme::Number(_)
                | Lexeme::True
                | Lexeme::False
                | Lexeme::Null => {
                    // `parse_scalar` lexes the next token before `numeric_in` reads a number.
                    let next = self.lex()?;
                    let token = match lexeme {
                        Lexeme::String(span) => Token::String(span),
                        Lexeme::Number(span) => Token::Number(
                            PgNumeric::parse(&self.text[span.start..span.end])
                                .map_err(JsonbError::Numeric)?,
                        ),
                        Lexeme::True => Token::Bool(true),
                        Lexeme::False => Token::Bool(false),
                        _ => Token::Null,
                    };
                    self.tokens.push(token);
                    lexeme = next;
                }
                other => return Err(expected(other, JsonbError::ExpectedValue)),
            }
            // A value ended and `lexeme` follows it.
            loop {
                let Some(open) = self.open.last_mut() else {
                    if lexeme != Lexeme::End {
                        return Err(JsonbError::ExpectedEnd);
                    }
                    return Ok(PgJsonb {
                        tokens: self.tokens,
                        strings: self.strings,
                    });
                };
                open.count += 1;
                let closed = *open;
                match (closed.object, lexeme) {
                    (false, Lexeme::Comma) => {
                        lexeme = self.lex()?;
                        continue 'value;
                    }
                    (true, Lexeme::Comma) => {
                        lexeme = self.lex()?;
                        let Lexeme::String(key) = lexeme else {
                            return Err(expected(lexeme, JsonbError::ExpectedString));
                        };
                        lexeme = self.key(key)?;
                        continue 'value;
                    }
                    (false, Lexeme::ArrayEnd) => {
                        lexeme = self.lex()?;
                        self.open.pop();
                        self.tokens[closed.start] = Token::ArrayStart(closed.count);
                        self.tokens.push(Token::ArrayEnd);
                    }
                    (true, Lexeme::ObjectEnd) => {
                        lexeme = self.lex()?;
                        self.open.pop();
                        self.close_object(closed);
                    }
                    (false, other) => {
                        return Err(expected(other, JsonbError::ExpectedCommaOrArrayEnd));
                    }
                    (true, other) => {
                        return Err(expected(other, JsonbError::ExpectedCommaOrObjectEnd));
                    }
                }
            }
        }
    }

    /// Opens a nonempty object or array, whose first key or element follows.
    fn open(&mut self, object: bool) {
        self.open.push(Open {
            object,
            start: self.tokens.len(),
            keys: self.keys.len(),
            count: 0,
        });
        self.tokens.push(if object {
            Token::ObjectStart(0)
        } else {
            Token::ArrayStart(0)
        });
    }

    /// Ends an object, its pairs put in storage order with the last of duplicate keys kept, as
    /// `uniqueifyJsonbObject` leaves them.
    fn close_object(&mut self, object: Open) {
        let keys = &self.keys[object.keys..];
        let text = |key: &Key| &self.strings[key.span.start..key.span.end];
        let ordered = keys
            .windows(2)
            .all(|pair| storage_order(text(&pair[0]), text(&pair[1])).is_lt());
        let count = if ordered {
            keys.len()
        } else {
            self.reorder_pairs(object.keys)
        };
        self.keys.truncate(object.keys);
        self.tokens[object.start] = Token::ObjectStart(count);
        self.tokens.push(Token::ObjectEnd);
    }

    /// Puts the pairs of the object whose keys start at `first` of [`Parser::keys`] in storage
    /// order, the last of duplicate keys kept, returning the pair count.
    fn reorder_pairs(&mut self, first: usize) -> usize {
        let keys = &self.keys[first..];
        let base = keys[0].token;
        let ends = keys[1..]
            .iter()
            .map(|key| key.token)
            .chain([self.tokens.len()]);
        // Each pair's key and its token range, counted from the first pair's.
        let mut pairs: Vec<(Span, usize, usize)> = keys
            .iter()
            .zip(ends)
            .map(|(key, end)| (key.span, key.token - base, end - base))
            .collect();
        let text = |span: Span| &self.strings[span.start..span.end];
        // The later of equal keys first, so that removing duplicates keeps it.
        pairs.sort_unstable_by(|left, right| {
            storage_order(text(left.0), text(right.0)).then(right.1.cmp(&left.1))
        });
        pairs.dedup_by(|later, kept| text(later.0) == text(kept.0));
        let mut tail = self.tokens.split_off(base);
        for &(_, start, end) in &pairs {
            self.tokens.extend(
                tail[start..end]
                    .iter_mut()
                    .map(|token| mem::replace(token, Token::Null)),
            );
        }
        pairs.len()
    }

    /// Reads an object's key, whose string was lexed, and its colon, returning the token that
    /// starts its value.
    fn key(&mut self, span: Span) -> Result<Lexeme, JsonbError> {
        self.keys.push(Key {
            token: self.tokens.len(),
            span,
        });
        self.tokens.push(Token::Key(span));
        let colon = self.lex()?;
        if colon != Lexeme::Colon {
            return Err(expected(colon, JsonbError::ExpectedColon));
        }
        self.lex()
    }

    /// `json_lex`, the next token after blanks.
    fn lex(&mut self) -> Result<Lexeme, JsonbError> {
        let bytes = self.text.as_bytes();
        while matches!(bytes.get(self.at), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
        let Some(&byte) = bytes.get(self.at) else {
            return Ok(Lexeme::End);
        };
        let punctuation = match byte {
            b'{' => Lexeme::ObjectStart,
            b'}' => Lexeme::ObjectEnd,
            b'[' => Lexeme::ArrayStart,
            b']' => Lexeme::ArrayEnd,
            b',' => Lexeme::Comma,
            b':' => Lexeme::Colon,
            b'"' => {
                self.at += 1;
                return self.string().map(Lexeme::String);
            }
            b'-' | b'0'..=b'9' => return self.number().map(Lexeme::Number),
            _ => {
                let start = self.at;
                let length = bytes[start..]
                    .iter()
                    .take_while(|&&byte| is_word_byte(byte))
                    .count();
                self.at += length;
                return match &bytes[start..self.at] {
                    b"true" => Ok(Lexeme::True),
                    b"false" => Ok(Lexeme::False),
                    b"null" => Ok(Lexeme::Null),
                    _ => Err(JsonbError::InvalidToken),
                };
            }
        };
        self.at += 1;
        Ok(punctuation)
    }

    /// `json_lex_number`, an optional `-`, a `0` or digits not starting with `0`, an optional
    /// point and digits, an optional exponent, and no word character after.
    fn number(&mut self) -> Result<Span, JsonbError> {
        let bytes = self.text.as_bytes();
        let start = self.at;
        let digits = |at: usize| {
            bytes[at..]
                .iter()
                .take_while(|b| b.is_ascii_digit())
                .count()
        };
        let mut at = start + usize::from(bytes[start] == b'-');
        let mut valid = true;
        match bytes.get(at) {
            Some(b'0') => at += 1,
            Some(b'1'..=b'9') => at += digits(at),
            _ => valid = false,
        }
        if bytes.get(at) == Some(&b'.') {
            at += 1;
            let fraction = digits(at);
            valid &= fraction > 0;
            at += fraction;
        }
        if matches!(bytes.get(at), Some(b'e' | b'E')) {
            at += 1;
            at += usize::from(matches!(bytes.get(at), Some(b'+' | b'-')));
            let exponent = digits(at);
            valid &= exponent > 0;
            at += exponent;
        }
        if !valid || bytes.get(at).copied().is_some_and(is_word_byte) {
            return Err(JsonbError::InvalidToken);
        }
        self.at = at;
        Ok(Span { start, end: at })
    }

    /// `json_lex_string` from after the opening quote, decoding the string into the parser's
    /// strings, the refusals checked in the order PostgreSQL checks them.
    fn string(&mut self) -> Result<Span, JsonbError> {
        let bytes = self.text.as_bytes();
        let start = self.strings.len();
        let mut high_surrogate: Option<u32> = None;
        loop {
            match bytes.get(self.at) {
                None => return Err(JsonbError::InvalidToken),
                Some(b'"') => {
                    self.at += 1;
                    if high_surrogate.is_some() {
                        return Err(JsonbError::LowSurrogateExpected);
                    }
                    return Ok(Span {
                        start,
                        end: self.strings.len(),
                    });
                }
                Some(b'\\') => {
                    self.at += 1;
                    match bytes.get(self.at) {
                        None => return Err(JsonbError::InvalidToken),
                        Some(b'u') => {
                            let mut code = 0;
                            for _ in 0..4 {
                                self.at += 1;
                                let digit = bytes.get(self.at).ok_or(JsonbError::InvalidToken)?;
                                code = code * 16
                                    + char::from(*digit)
                                        .to_digit(16)
                                        .ok_or(JsonbError::UnicodeEscapeFormat)?;
                            }
                            self.at += 1;
                            if (0xD800..0xDC00).contains(&code) {
                                if high_surrogate.is_some() {
                                    return Err(JsonbError::HighSurrogateRepeated);
                                }
                                high_surrogate = Some(code);
                                continue;
                            }
                            if (0xDC00..0xE000).contains(&code) {
                                let high = high_surrogate
                                    .take()
                                    .ok_or(JsonbError::LowSurrogateExpected)?;
                                code = 0x10000 + ((high - 0xD800) << 10) + (code - 0xDC00);
                            }
                            if high_surrogate.is_some() {
                                return Err(JsonbError::LowSurrogateExpected);
                            }
                            if code == 0 {
                                return Err(JsonbError::CodePointZero);
                            }
                            self.strings.push(
                                char::from_u32(code).expect("a scalar value after surrogates"),
                            );
                        }
                        Some(&escaped) => {
                            if high_surrogate.is_some() {
                                return Err(JsonbError::LowSurrogateExpected);
                            }
                            self.strings.push(match escaped {
                                b'"' => '"',
                                b'\\' => '\\',
                                b'/' => '/',
                                b'b' => '\u{8}',
                                b'f' => '\u{c}',
                                b'n' => '\n',
                                b'r' => '\r',
                                b't' => '\t',
                                _ => return Err(JsonbError::InvalidEscape),
                            });
                            self.at += 1;
                        }
                    }
                }
                Some(_) => {
                    if high_surrogate.is_some() {
                        return Err(JsonbError::LowSurrogateExpected);
                    }
                    let run = self.at;
                    while let Some(&byte) = bytes.get(self.at) {
                        match byte {
                            b'"' | b'\\' => break,
                            0..0x20 => return Err(JsonbError::UnescapedControlCharacter),
                            _ => self.at += 1,
                        }
                    }
                    self.strings.push_str(&self.text[run..self.at]);
                }
            }
        }
    }
}

/// `escape_json`, the string quoted with `"`, `\` and control characters escaped and every
/// other character written as it is.
fn write_string(f: &mut fmt::Formatter<'_>, text: &str) -> fmt::Result {
    f.write_char('"')?;
    let mut written = 0;
    for (at, byte) in text.bytes().enumerate() {
        let escape = match byte {
            b'"' => Some("\\\""),
            b'\\' => Some("\\\\"),
            0x08 => Some("\\b"),
            0x0C => Some("\\f"),
            b'\n' => Some("\\n"),
            b'\r' => Some("\\r"),
            b'\t' => Some("\\t"),
            0..0x20 => None,
            _ => continue,
        };
        f.write_str(&text[written..at])?;
        match escape {
            Some(escape) => f.write_str(escape)?,
            None => write!(f, "\\u{byte:04x}")?,
        }
        written = at + 1;
    }
    f.write_str(&text[written..])?;
    f.write_char('"')
}

/// `JsonbToCString`, keys and values joined by `": "`, pairs and elements by `", "`.
impl fmt::Display for PgJsonb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for token in &self.tokens {
            if !first && !matches!(token, Token::ArrayEnd | Token::ObjectEnd) {
                f.write_str(", ")?;
            }
            first = false;
            match token {
                Token::Null => f.write_str("null")?,
                Token::Bool(value) => f.write_str(if *value { "true" } else { "false" })?,
                Token::Number(value) => write!(f, "{value}")?,
                Token::String(span) => write_string(f, self.text(*span))?,
                Token::Key(span) => {
                    write_string(f, self.text(*span))?;
                    f.write_str(": ")?;
                    first = true;
                }
                Token::ArrayStart(_) => {
                    f.write_char('[')?;
                    first = true;
                }
                Token::ObjectStart(_) => {
                    f.write_char('{')?;
                    first = true;
                }
                Token::ArrayEnd => f.write_char(']')?,
                Token::ObjectEnd => f.write_char('}')?,
            }
        }
        Ok(())
    }
}

impl PartialEq for PgJsonb {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for PgJsonb {}

impl PartialOrd for PgJsonb {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// `compareJsonbContainers`, both token streams walked together to the first difference. A
/// top-level scalar is a one-element pseudo array there, so it sorts after the empty array and
/// before every other array and object.
impl Ord for PgJsonb {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.scalar(), other.scalar()) {
            (Some(left), Some(right)) => self.cmp_token(left, other, right),
            (Some(_), None) => match other.tokens[0] {
                Token::ArrayStart(0) => Ordering::Greater,
                _ => Ordering::Less,
            },
            (None, Some(_)) => match self.tokens[0] {
                Token::ArrayStart(0) => Ordering::Less,
                _ => Ordering::Greater,
            },
            (None, None) => self
                .tokens
                .iter()
                .zip(&other.tokens)
                .map(|(left, right)| self.cmp_token(left, other, right))
                .find(|order| order.is_ne())
                .unwrap_or_else(|| self.tokens.len().cmp(&other.tokens.len())),
        }
    }
}

impl Hash for PgJsonb {
    fn hash<H: Hasher>(&self, state: &mut H) {
        for token in &self.tokens {
            mem::discriminant(token).hash(state);
            match token {
                Token::Bool(value) => value.hash(state),
                Token::Number(value) => value.hash(state),
                Token::String(span) | Token::Key(span) => self.text(*span).hash(state),
                Token::ArrayStart(count) | Token::ObjectStart(count) => count.hash(state),
                Token::Null | Token::ArrayEnd | Token::ObjectEnd => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use super::*;

    #[test]
    fn nesting_of_any_depth_parses_prints_compares_and_drops() {
        let depth = 200_000;
        let text = "[".repeat(depth) + &"]".repeat(depth);
        let deep = PgJsonb::parse(&text).unwrap();
        assert_eq!(deep.to_string(), text);
        let shallower = PgJsonb::parse(&text[1..text.len() - 1]).unwrap();
        assert!(shallower < deep);
        assert_eq!(deep.clone(), deep);
    }
}
