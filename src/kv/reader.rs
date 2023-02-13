use std::borrow::Borrow;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;
use std::hash::Hash;
use std::io::Read;

use bumpalo::collections::String;
use bumpalo::Bump;
use ouroboros::self_referencing;

use super::char_reader::{CharReader, ReadChar};

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

#[self_referencing]
pub struct KeyValues {
    allocator: Bump,

    #[borrows(allocator)]
    #[covariant]
    root: Object<'this>,
}

/// Represents a generic KV object.
#[derive(Debug, Default)]
pub struct Object<'a> {
    kv: HashMap<String<'a>, (Flag<'a>, Value<'a>)>,
}

/// Represents a generic KV value.
#[derive(Debug)]
pub enum Value<'a> {
    String(String<'a>),
    Object(Object<'a>),
}

/// Represents a KV entry flag
#[derive(Debug)]
pub enum Flag<'a> {
    None,
    Normal(String<'a>),
    Negated(String<'a>),
}

impl KeyValues {
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
    pub fn from_io<'c, 'b: 'c, R: Read>(read: R) -> Result<KeyValues> {
        let mut char_reader = CharReader::from_io(read)?;

        KeyValuesTryBuilder {
            allocator: Bump::with_capacity(1024),
            root_builder: |allocator: &Bump| Self::visit_object(&mut char_reader, allocator),
        }
        .try_build()
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

    fn visit_text_quoted<'bump, R: Read>(
        char_reader: &mut CharReader<R>,
        allocator: &'bump Bump,
    ) -> Result<String<'bump>> {
        debug_assert!(char_reader.peek() == ReadChar::Normal(QUOTE));
        Self::advance(char_reader)?;

        let mut read_string = String::with_capacity_in(BASE_STRING_SIZE, allocator);

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

    fn visit_text_unquoted<'bump, R: Read>(
        char_reader: &mut CharReader<R>,
        allocator: &'bump Bump,
    ) -> Result<String<'bump>> {
        debug_assert!(Self::is_unquoted_text_char(&char_reader.peek()));

        let mut read_string = String::with_capacity_in(BASE_STRING_SIZE, allocator);

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

    fn visit_text<'bump, R: Read>(
        char_reader: &mut CharReader<R>,
        allocator: &'bump Bump,
    ) -> Result<String<'bump>> {
        debug_assert!(char_reader.peek().is_char());

        if char_reader.peek().unwrap_char() == QUOTE {
            Self::visit_text_quoted(char_reader, allocator)
        } else {
            Self::visit_text_unquoted(char_reader, allocator)
        }
    }

    fn visit_flag<'bump, R: Read>(
        char_reader: &mut CharReader<R>,
        allocator: &'bump Bump,
    ) -> Result<Flag<'bump>> {
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

        let mut read_string = String::with_capacity_in(BASE_STRING_SIZE, allocator);

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

    fn visit_value<'bump, R: Read>(
        char_reader: &mut CharReader<R>,
        allocator: &'bump Bump,
    ) -> Result<Value<'bump>> {
        let read = char_reader.peek();
        if read == ReadChar::Normal(OPEN_BLOCK) {
            Self::visit_open(char_reader)?;
            let object = Self::visit_object(char_reader, allocator)?;
            Self::visit_close(char_reader)?;

            Ok(Value::Object(object))
        } else if Self::is_unquoted_text_char(&read) || matches!(read, ReadChar::Normal(QUOTE)) {
            let text = Self::visit_text(char_reader, allocator)?;

            Ok(Value::String(text))
        } else {
            Err(ReaderError::InvalidChar(char_reader.peek()))
        }
    }

    fn visit_object<'bump, R: Read>(
        char_reader: &mut CharReader<R>,
        allocator: &'bump Bump,
    ) -> Result<Object<'bump>> {
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

            let key = Self::visit_text(char_reader, allocator)?;
            let value = Self::visit_value(char_reader, allocator)?;
            let flag = Self::visit_flag(char_reader, allocator)?;

            new_obj.kv.insert(key, (flag, value));
        }

        Ok(new_obj)
    }

    pub fn get<Q: ?Sized>(&self, k: &Q) -> Option<&Value>
    where
        for<'b> String<'b>: Borrow<Q>,
        Q: Hash + Eq,
    {
        self.borrow_root().get(k)
    }

    pub fn get_with_flags<Q: ?Sized, T: Sized>(&self, k: &Q, flags: HashSet<T>) -> Option<&Value>
    where
        for<'b> String<'b>: Borrow<Q>,
        Q: Hash + Eq,
        for<'b> T: Borrow<String<'b>>,
        T: Hash + Eq,
    {
        self.borrow_root().get_with_flags(k, flags)
    }
}

impl<'a> Object<'a> {
    pub fn get<Q: ?Sized>(&self, k: &Q) -> Option<&Value>
    where
        String<'a>: Borrow<Q>,
        Q: Hash + Eq,
    {
        match self.kv.get(k) {
            None => None,
            Some(f_v) => Some(&f_v.1),
        }
    }

    pub fn get_with_flags<Q: ?Sized, T: Sized>(&self, k: &Q, flags: HashSet<T>) -> Option<&Value>
    where
        String<'a>: Borrow<Q>,
        Q: Hash + Eq,
        T: Borrow<String<'a>>,
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
    use super::{KeyValues, Value};

    fn string_matches(val: &Value, expected: &str) -> bool {
        match val {
            Value::String(v) => v == expected,
            _ => false,
        }
    }

    #[test]
    fn single_kv() {
        let object = KeyValues::from_io(r#"key "val""#.as_bytes()).unwrap();

        assert!(string_matches(object.get("key").unwrap(), "val"));
    }

    #[test]
    fn double_kv() {
        let kv = r#"
        key1 val1
        "key2" "val2"
        "#
        .as_bytes();

        let object = KeyValues::from_io(kv).unwrap();

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

        let object = KeyValues::from_io(kv).unwrap();

        match object.get("comp").unwrap() {
            Value::Object(comp) => {
                assert!(string_matches(comp.get("key1").unwrap(), "val1"));
                assert!(string_matches(comp.get("key2").unwrap(), "val2"));
            }
            _ => panic!(),
        }
    }
}
