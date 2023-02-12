use std::borrow::Borrow;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;
use std::hash::Hash;
use std::io::Read;

use super::char_reader::{CharReader, ReadChar};

#[derive(Debug, PartialEq)]
pub enum Token {
    OpenBlock,
    CloseBlock,
    Text(String),
    Eof,
}

const BASE_STRING_SIZE: usize = 1024;
const QUOTE: char = '"';
const OPEN_BLOCK: char = '{';
const CLOSE_BLOCK: char = '}';
const OPEN_FLAG: char = '[';
const CLOSE_FLAG: char = ']';
const NEGATE: char = '!';

#[derive(Debug)]
pub enum ReaderError {
    IO(std::io::Error),
    InvalidChar(ReadChar),
    UnexpectedEof,
}
pub type Result<T> = std::result::Result<T, ReaderError>;

impl From<std::io::Error> for ReaderError {
    fn from(err: std::io::Error) -> ReaderError {
        ReaderError::IO(err)
    }
}

impl fmt::Display for ReaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReaderError::IO(err) => {
                write!(f, "IO error encountered in reading:\n\t{}", err.to_string())
            }
            ReaderError::InvalidChar(data) => write!(f, "Invalid char: {data:?}"),
            ReaderError::UnexpectedEof => write!(f, "Unexpected EOF"),
        }
    }
}

impl Error for ReaderError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            ReaderError::IO(ref err) => Some(err),
            ReaderError::InvalidChar(_) => None,
            ReaderError::UnexpectedEof => None,
        }
    }
}

/// Represents a generic KV object.
#[derive(Debug, Default)]
pub struct Object {
    kv: HashMap<String, (Flag, Value)>,
}

/// Represents a generic KV value.
#[derive(Debug)]
pub enum Value {
    String(String),
    Object(Object),
}

/// Represents a KV entry flag
#[derive(Debug)]
pub enum Flag {
    None,
    Normal(String),
    Negated(String),
}

impl Object {
    /// Parses a Keyvalues object from an `std::io::Read` object.
    /// # Examples
    /// ```
    /// use srcrs::kv::{Object, Value};
    ///
    /// let kv = r#"
    ///    comp {
    ///        key1 val1
    ///        key2 val2
    ///    }
    /// "#.as_bytes();
    ///
    /// let object = Object::from_io(kv).unwrap();
    ///
    /// match object.kv.get("comp").unwrap() {
    ///     Value::Object(comp) => {
    ///         assert!(*comp.kv.get("key1").unwrap() == Value::String("val1".into()));
    ///         assert!(*comp.kv.get("key2").unwrap() == Value::String("val2".into()));
    ///     }
    ///     _ => panic!(),
    /// }
    /// ```
    pub fn from_io<R: Read>(read: R) -> Result<Object> {
        let mut char_reader = CharReader::from_io(read)?;

        Ok(Self::visit_object(&mut char_reader)?)
    }

    #[inline]
    fn is_unquoted_text_char(data: &ReadChar) -> bool {
        match data {
            ReadChar::Normal(c_data) => match *c_data {
                OPEN_BLOCK | CLOSE_BLOCK | OPEN_FLAG | QUOTE => false,
                _ => !c_data.is_whitespace(),
            },
            ReadChar::Escaped(_) => true,
            _ => false,
        }
    }

    #[inline]
    fn advance<R: Read>(char_reader: &mut CharReader<R>) -> Result<()> {
        char_reader.advance()?;
        Ok(())
    }

    #[inline]
    fn advance_whitespace<R: Read>(char_reader: &mut CharReader<R>) -> Result<()> {
        char_reader.advance()?;
        if matches!(char_reader.peek(), ReadChar::Whitespace) {
            char_reader.advance()?;
        }

        Ok(())
    }

    #[inline]
    fn visit_open<R: Read>(char_reader: &mut CharReader<R>) -> Result<()> {
        debug_assert!(char_reader.peek() == ReadChar::Normal(OPEN_BLOCK));
        Self::advance_whitespace(char_reader)?;

        Ok(())
    }

    #[inline]
    fn visit_close<R: Read>(char_reader: &mut CharReader<R>) -> Result<()> {
        debug_assert!(char_reader.peek() == ReadChar::Normal(CLOSE_BLOCK));
        Self::advance_whitespace(char_reader)?;

        Ok(())
    }

    fn visit_text_quoted<R: Read>(char_reader: &mut CharReader<R>) -> Result<String> {
        debug_assert!(char_reader.peek() == ReadChar::Normal(QUOTE));
        Self::advance(char_reader)?;

        let mut read_string = String::with_capacity(BASE_STRING_SIZE);

        while char_reader.peek() != ReadChar::Normal(QUOTE) {
            let read_peek = char_reader.peek();

            if matches!(read_peek, ReadChar::Eof) {
                return Err(ReaderError::UnexpectedEof);
            }

            read_string.push(read_peek.unwrap_char());
            Self::advance(char_reader)?;
        }
        Self::advance_whitespace(char_reader)?;

        read_string.shrink_to_fit();
        Ok(read_string)
    }

    fn visit_text_unquoted<R: Read>(char_reader: &mut CharReader<R>) -> Result<String> {
        debug_assert!(Self::is_unquoted_text_char(&char_reader.peek()));

        let mut read_string = String::with_capacity(BASE_STRING_SIZE);

        while Self::is_unquoted_text_char(&char_reader.peek()) {
            read_string.push(char_reader.peek().unwrap_char());
            Self::advance(char_reader)?;
        }

        if matches!(char_reader.peek(), ReadChar::Whitespace) {
            Self::advance(char_reader)?;
        }

        read_string.shrink_to_fit();
        Ok(read_string)
    }

    fn visit_text<R: Read>(char_reader: &mut CharReader<R>) -> Result<String> {
        debug_assert!(char_reader.peek().is_char());

        if char_reader.peek().unwrap_char() == QUOTE {
            Self::visit_text_quoted(char_reader)
        } else {
            Self::visit_text_unquoted(char_reader)
        }
    }

    fn visit_flag<R: Read>(char_reader: &mut CharReader<R>) -> Result<Flag> {
        let first_read = char_reader.peek();
        if first_read != ReadChar::Normal(OPEN_FLAG) {
            debug_assert!(
                first_read.is_eof()
                    || (first_read.is_char()
                        && (first_read.unwrap_char() == QUOTE
                            || first_read.unwrap_char() == CLOSE_BLOCK
                            || Self::is_unquoted_text_char(&first_read)))
            );

            return Ok(Flag::None);
        }

        debug_assert!(char_reader.peek() == ReadChar::Normal(OPEN_FLAG));
        Self::advance_whitespace(char_reader)?;

        let is_negated = char_reader.peek() == ReadChar::Normal(NEGATE);
        if is_negated {
            Self::advance_whitespace(char_reader)?;
        }

        let mut read_string = String::with_capacity(BASE_STRING_SIZE);

        while char_reader.peek() != ReadChar::Normal(CLOSE_FLAG) {
            let read_peek = char_reader.peek();

            if matches!(read_peek, ReadChar::Eof) {
                return Err(ReaderError::UnexpectedEof);
            }

            read_string.push(read_peek.unwrap_char());
            Self::advance(char_reader)?;
        }

        if matches!(char_reader.peek(), ReadChar::Whitespace) {
            Self::advance(char_reader)?;
        }

        Self::advance_whitespace(char_reader)?;

        read_string.shrink_to_fit();

        if is_negated {
            Ok(Flag::Negated(read_string))
        } else {
            Ok(Flag::Normal(read_string))
        }
    }

    fn visit_value<R: Read>(char_reader: &mut CharReader<R>) -> Result<Value> {
        let read = char_reader.peek();
        if read == ReadChar::Normal(OPEN_BLOCK) {
            Self::visit_open(char_reader)?;
            let object = Self::visit_object(char_reader)?;
            Self::visit_close(char_reader)?;

            Ok(Value::Object(object))
        } else if Self::is_unquoted_text_char(&read) || matches!(read, ReadChar::Normal(QUOTE)) {
            let text = Self::visit_text(char_reader)?;

            Ok(Value::String(text))
        } else {
            Err(ReaderError::InvalidChar(char_reader.peek()))
        }
    }

    fn visit_object<R: Read>(char_reader: &mut CharReader<R>) -> Result<Object> {
        let mut new_obj = Object::default();

        while char_reader.peek() != ReadChar::Eof {
            let peeked_char = char_reader.peek();

            if peeked_char.is_char() {
                if peeked_char.unwrap_char() == CLOSE_BLOCK {
                    break;
                }

                if peeked_char.unwrap_char() != QUOTE && !Self::is_unquoted_text_char(&peeked_char)
                {
                    return Err(ReaderError::InvalidChar(peeked_char));
                }
            } else {
                return Err(ReaderError::InvalidChar(peeked_char));
            }

            let key = Self::visit_text(char_reader)?;
            let value = Self::visit_value(char_reader)?;
            let flag = Self::visit_flag(char_reader)?;

            new_obj.kv.insert(key, (flag, value));
        }

        Ok(new_obj)
    }

    pub fn get<Q: ?Sized>(&self, k: &Q) -> Option<&Value>
    where
        String: Borrow<Q>,
        Q: Hash + Eq,
    {
        match self.kv.get(k) {
            None => None,
            Some(f_v) => Some(&f_v.1),
        }
    }

    pub fn get_with_flags<Q: ?Sized, T: Sized>(&self, k: &Q, flags: HashSet<T>) -> Option<&Value>
    where
        String: Borrow<Q>,
        Q: Hash + Eq,
        T: Borrow<String>,
        T: Hash + Eq,
    {
        match self.kv.get(k) {
            None => return None,
            Some(f_v) => match &f_v.0 {
                Flag::None => Some(&f_v.1),
                Flag::Normal(flag) => {
                    if flags.contains(&flag) {
                        Some(&f_v.1)
                    } else {
                        None
                    }
                }
                Flag::Negated(flag) => {
                    if !flags.contains(&flag) {
                        Some(&f_v.1)
                    } else {
                        None
                    }
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Object, Value};

    fn string_matches(val: &Value, expected: &str) -> bool {
        match val {
            Value::String(v) => v == expected,
            _ => false,
        }
    }

    #[test]
    fn single_kv() {
        let object = Object::from_io(r#"key "val""#.as_bytes()).unwrap();

        assert!(string_matches(object.get("key").unwrap(), "val"));
    }

    #[test]
    fn double_kv() {
        let kv = r#"
        key1 val1
        "key2" "val2"
        "#
        .as_bytes();

        let object = Object::from_io(kv).unwrap();

        assert!(string_matches(object.get("key1").unwrap(), "val1"));
        assert!(string_matches(object.get("key2").unwrap(), "val2"));
    }

    #[test]
    fn compound_kv() {
        let kv = r#"
        comp {
            key1 val1
            key2 val2
        }
        "#
        .as_bytes();

        let object = Object::from_io(kv).unwrap();

        match object.get("comp").unwrap() {
            Value::Object(comp) => {
                assert!(string_matches(comp.get("key1").unwrap(), "val1"));
                assert!(string_matches(comp.get("key2").unwrap(), "val2"));
            }
            _ => panic!(),
        }
    }
}
