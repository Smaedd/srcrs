use std::{io::Read, error::Error};
use std::fmt;
use std::collections::HashMap;

use super::tokenizer::{Tokenizer, TokenizerError, Token};

#[derive(Debug)]
pub enum ReaderError {
    Tokenizer(TokenizerError),
    InvalidToken(Token),
}
pub type Result<T> = std::result::Result<T, ReaderError>;

impl From<TokenizerError> for ReaderError {
    fn from(err: TokenizerError) -> ReaderError {
        ReaderError::Tokenizer(err)
    }
}

impl fmt::Display for ReaderError {
    fn fmt(&self, f:&mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReaderError::Tokenizer(err) => write!(f, "Tokenizer error enounctered in reading:\n\t{}", err.to_string()),
            ReaderError::InvalidToken(token) => write!(f, "Invalid token: {token:?}"),
        }
    }
}

impl Error for ReaderError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            ReaderError::Tokenizer(ref err) => Some(err),
            ReaderError::InvalidToken(_) => None,
        }
    }
}

/// Represents a generic KV object.
#[derive(Debug, Default, PartialEq)]
pub struct Object {
    pub kv: HashMap<String, Value>
}

/// Represents a generic KV value.
#[derive(Debug, PartialEq)]
pub enum Value {
    String(String),
    Object(Object),
}

macro_rules! expect {
    ($expression:expr, $pattern:pat_param, $resolve:expr) => {
        {
            let value = $expression;
            match value {
                $pattern => $resolve,
                _ => return Err(ReaderError::InvalidToken(value))
            }
        }
    };
    ($expression:expr, $pattern:pat_param) => {
        expect!($expression, $pattern, ())
    };
}


impl Object {

    /// Parses a Keyvalues object from an `std::io::Read` object.
    /// # Examples
    /// ```
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
    pub fn from_io<R: Read>(read: R) -> Result<Object>{
        let mut tokenizer = Tokenizer::from_io(read)?;

        let mut outer_object = Object::default();
        
        loop {
            let attemped_entry = Self::read_entry(&mut tokenizer);

            match attemped_entry {
                Err(ReaderError::InvalidToken(Token::Eof)) => break,
                Err(err) => return Err(err),
                Ok((k, v)) => outer_object.kv.insert(k, v),
            };
        }

        Ok(outer_object)
    }

    fn read_entry<R: Read>(tokenizer: &mut Tokenizer<R>) -> Result<(String, Value)>
    {
        let key = expect!(tokenizer.next_token()?, Token::Text(text), text);

        let val_token = tokenizer.next_token()?;
        match val_token {
            Token::Text(val) => Ok((key, Value::String(val))),
            Token::OpenBlock => {
                let mut object = Object::default();

                loop {
                    match Self::read_entry(tokenizer) {
                        Err(ReaderError::InvalidToken(Token::CloseBlock)) => break,
                        Err(err) => return Err(err),
                        Ok((k, v)) => object.kv.insert(k, v),
                    };
                }

                Ok((key, Value::Object(object)))
            },
            _ => Err(ReaderError::InvalidToken(val_token)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Object, Value};

    #[test]
    fn single_kv() {
        let object = Object::from_io(r#"key "val""#.as_bytes()).unwrap();

        assert!(*object.kv.get("key").unwrap() == Value::String("val".into()));
    }

    #[test]
    fn double_kv() {
        let kv = r#"
        key1 val1
        "key2" "val2"
        "#.as_bytes();

        let object = Object::from_io(kv).unwrap();

        assert!(*object.kv.get("key1").unwrap() == Value::String("val1".into()));
        assert!(*object.kv.get("key2").unwrap() == Value::String("val2".into()));
    }

    #[test]
    fn compound_kv() {
        let kv = r#"
        comp {
            key1 val1
            key2 val2
        }
        "#.as_bytes();

        let object = Object::from_io(kv).unwrap();

        match object.kv.get("comp").unwrap() {
            Value::Object(comp) => {
                assert!(*comp.kv.get("key1").unwrap() == Value::String("val1".into()));
                assert!(*comp.kv.get("key2").unwrap() == Value::String("val2".into()));
            }
            _ => panic!(),
        }
    }
}
