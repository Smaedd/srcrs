use std::borrow::Borrow;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;
use std::hash::Hash;
use std::io::Read;
use std::mem;

use bumpalo::collections::String;
use bumpalo::Bump;
use ouroboros::self_referencing;

use super::token_reader::{self, Token, TokenReader};

const BASE_STRING_SIZE: usize = 1024;

#[derive(Debug)]
pub enum ReaderError {
    IO(std::io::Error),
    InvalidToken(std::string::String),
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
            ReaderError::InvalidToken(data) => write!(f, "Invalid token: {data}"),
            ReaderError::UnexpectedEof => write!(f, "Unexpected EOF"),
        }
    }
}

impl Error for ReaderError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            ReaderError::IO(ref err) => Some(err),
            ReaderError::InvalidToken(_) => None,
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
        KeyValuesTryBuilder {
            allocator: Bump::with_capacity(1024),
            root_builder: |allocator: &Bump| {
                let mut token_reader = TokenReader::from_io(read, allocator)?;
                Self::visit_object(&mut token_reader)
            },
        }
        .try_build()
    }

    #[inline]
    fn visit_open_block<'bump, R: Read>(token_reader: &mut TokenReader<'bump, R>) -> Result<()> {
        debug_assert!(*token_reader.peek() == Token::OpenBlock);
        token_reader.advance()?;

        Ok(())
    }

    #[inline]
    fn visit_close_block<'bump, R: Read>(token_reader: &mut TokenReader<'bump, R>) -> Result<()> {
        debug_assert!(*token_reader.peek() == Token::CloseBlock);
        token_reader.advance()?;

        Ok(())
    }

    #[inline]
    fn visit_open_flag<'bump, R: Read>(token_reader: &mut TokenReader<'bump, R>) -> Result<()> {
        debug_assert!(*token_reader.peek() == Token::OpenFlag);
        token_reader.advance()?;

        Ok(())
    }

    #[inline]
    fn visit_close_flag<'bump, R: Read>(token_reader: &mut TokenReader<'bump, R>) -> Result<()> {
        debug_assert!(*token_reader.peek() == Token::CloseFlag);
        token_reader.advance()?;

        Ok(())
    }

    #[inline]
    fn visit_flag_negation<'bump, R: Read>(
        token_reader: &mut TokenReader<'bump, R>,
    ) -> Result<bool> {
        if matches!(*token_reader.peek(), Token::Negate) {
            token_reader.advance()?;
            return Ok(true);
        }

        Ok(false)
    }

    #[inline]
    fn visit_text<'bump, R: Read>(
        token_reader: &mut TokenReader<'bump, R>,
    ) -> Result<String<'bump>> {
        debug_assert!(matches!(*token_reader.peek(), Token::Text(_)));

        let text = token_reader.peek().unwrap_text();
        token_reader.advance()?;
        Ok(text)
    }

    fn visit_flag<'bump, R: Read>(token_reader: &mut TokenReader<'bump, R>) -> Result<Flag<'bump>> {
        if !matches!(token_reader.peek(), Token::OpenFlag) {
            return Ok(Flag::None);
        }

        Self::visit_open_flag(token_reader)?;
        let negated = Self::visit_flag_negation(token_reader)?;
        let text = Self::visit_text(token_reader)?;
        Self::visit_close_flag(token_reader)?;

        if negated {
            Ok(Flag::Negated(text))
        } else {
            Ok(Flag::Normal(text))
        }
    }

    fn visit_value<'bump, R: Read>(
        token_reader: &mut TokenReader<'bump, R>,
    ) -> Result<Value<'bump>> {
        match token_reader.peek() {
            Token::OpenBlock => {
                Self::visit_open_block(token_reader)?;
                let object = Self::visit_object(token_reader)?;
                Self::visit_close_block(token_reader)?;

                Ok(Value::Object(object))
            }
            Token::Text(text) => {
                let moved = mem::replace(text, String::new_in(text.bump()));

                token_reader.advance()?;
                Ok(Value::String(moved))
            }
            _ => Err(ReaderError::InvalidToken(format!(
                "{:?}",
                *token_reader.peek()
            ))),
        }
    }

    fn visit_object<'bump, R: Read>(
        token_reader: &mut TokenReader<'bump, R>,
    ) -> Result<Object<'bump>> {
        let mut new_obj = Object::default();

        while !matches!(token_reader.peek(), Token::Eof) {
            match token_reader.peek() {
                Token::CloseBlock => break,
                Token::Text(_) => {
                    let key = Self::visit_text(token_reader)?;
                    let value = Self::visit_value(token_reader)?;
                    let flag = Self::visit_flag(token_reader)?;

                    new_obj.kv.insert(key, (flag, value));
                }
                _ => {
                    return Err(ReaderError::InvalidToken(format!(
                        "{:?}",
                        *token_reader.peek()
                    )))
                }
            }
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
