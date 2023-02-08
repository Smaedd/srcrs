use std::{io::Read, error::Error};
use std::fmt;

#[derive(Debug)]
pub enum TokenizerError {
    IOError(std::io::Error)
}
pub type Result<T> = std::result::Result<T, TokenizerError>;

impl From<std::io::Error> for TokenizerError {
    fn from(err: std::io::Error) -> TokenizerError {
        TokenizerError::IOError(err)
    }
}

impl fmt::Display for TokenizerError {
    fn fmt(&self, f:&mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenizerError::IOError(err) => write!(f, "IO Error encountered in tokenization:\n\t{}", err.to_string()),
        }
    }
}

impl Error for TokenizerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            TokenizerError::IOError(ref err) => Some(err),
        }
    }
}

const READ_SIZE: usize = 1024;

pub struct Tokenizer<R>
where
    R: Read,
{
    reader: R,

    last_read: [u8; READ_SIZE],
    position: usize,
    max_read: usize
}

#[derive(Debug, PartialEq)]
pub enum Token {
    OpenBlock,
    CloseBlock,
    Text(String),
    Eof,
}

const BASE_STRING_SIZE: usize = 1024;
const QUOTE: char = '"';
const CANCEL: char = '\\';
const OPEN_BLOCK: char = '{';
const CLOSE_BLOCK: char = '}';


impl<R: Read> Tokenizer<R> {
    pub fn from_io(mut read: R) -> Result<Self> {
        let mut last_read = [0u8; READ_SIZE];
        let max_read: usize = read.read(last_read.as_mut_slice())?;

        Ok(Self {
            reader: read,

            last_read: last_read,
            position: 0,
            max_read: max_read,
        })
    }

    fn advance(&mut self) -> Result<()>{
        self.position += 1;

        if self.position >= self.max_read {
            self.max_read = self.reader.read(self.last_read.as_mut_slice())?;
            self.position = 0;
        }

        Ok(())
    }

    fn peek(&self) -> Option<char> {
        if self.max_read == 0 {
            return None;
        }

        return Some(self.last_read[self.position] as char);
    }

    fn consume_whitespace(&mut self) -> Result<()> {
        while let Some(data) = self.peek() {
            if !data.is_whitespace() {
                break;
            }

            self.advance()?;
        }

        Ok(())
    }

    pub fn next_token(&mut self) -> Result<Token>{
        self.consume_whitespace()?;

        match self.peek() {
            None => return Ok(Token::Eof),
            Some(first) => match first {
                OPEN_BLOCK => {
                    self.advance()?;
                    return Ok(Token::OpenBlock);
                }
                CLOSE_BLOCK => {
                    self.advance()?;
                    return Ok(Token::CloseBlock);
                }
                QUOTE => {
                    return Ok(Token::Text(self.read_quote_string()?));
                }
                _ => {
                    return Ok(Token::Text(self.read_quoteless_string()?));
                }
            }
        }
    }

    fn is_special_character(data: char) -> bool {
        match data {
            OPEN_BLOCK | CLOSE_BLOCK => true,
            _ => false,
        }
    }

    fn read_quote_string(&mut self) -> Result<String> {
        // Skip over first quote
        self.advance()?;

        let mut string = String::with_capacity(BASE_STRING_SIZE);

        let mut cancelled = false;
        loop {
            match self.peek() {
                None => break,
                Some(data) => {

                    if cancelled {
                        cancelled = false;
                    } else {
                        if data == QUOTE {
                            self.advance()?;
                            break;
                        } else if data == CANCEL {
                            cancelled = true;
                            self.advance()?;
                            continue;
                        }
                    }

                    self.advance()?;
                    string.push(data);
                }
            }
        }

        string.shrink_to_fit();
        Ok(string)
    }

    fn read_quoteless_string(&mut self) -> Result<String> {
        let mut string = String::with_capacity(BASE_STRING_SIZE);

        let mut cancelled = false;
        loop {
            match self.peek() {
                None => break,
                Some(data) => {

                    if cancelled {
                        cancelled = false;
                    } else {
                        if data.is_whitespace() {
                            self.advance()?;
                            break;
                        } else if data == CANCEL {
                            cancelled = true;
                            self.advance()?;
                            continue;
                        } else if Self::is_special_character(data) {
                            break;
                        }
                    }

                    self.advance()?;
                    string.push(data);
                }
            }
        }

        string.shrink_to_fit();
        Ok(string)
    }
}

#[cfg(test)]
mod tests {
    use super::{Token, Tokenizer};

    #[test]
    fn empty_input() {
        let mut tokenizer = Tokenizer::from_io(r#""#.as_bytes()).unwrap();

        assert!(tokenizer.next_token().unwrap() == Token::Eof);
    }

    #[test]
    fn regular_single_string() {
        let mut tokenizer = Tokenizer::from_io(r#"hello"#.as_bytes()).unwrap();

        assert!(tokenizer.next_token().unwrap() == Token::Text("hello".into()));
    }

    #[test]
    fn quoted_single_string() {
        let mut tokenizer = Tokenizer::from_io(r#""hey""#.as_bytes()).unwrap();

        assert!(tokenizer.next_token().unwrap() == Token::Text("hey".into()));
    }

    #[test]
    fn escaped_single_string() {
        let mut tokenizer = Tokenizer::from_io(r#"\"he\y\\\""#.as_bytes()).unwrap();

        assert!(tokenizer.next_token().unwrap() == Token::Text(r#""hey\""#.into()));
    }

    #[test]
    fn quoted_escaped_single_string() {
        let mut tokenizer = Tokenizer::from_io(r#""hey\"""#.as_bytes()).unwrap();

        assert!(tokenizer.next_token().unwrap() == Token::Text(r#"hey""#.into()));
    }

    #[test]
    fn whitespace_single_string() {
        let mut tokenizer = Tokenizer::from_io(r#"      "  hey  "          "#.as_bytes()).unwrap();

        assert!(tokenizer.next_token().unwrap() == Token::Text(r#"  hey  "#.into()));
    }

    #[test]
    fn open_brace() {
        let mut tokenizer = Tokenizer::from_io(r#"{"#.as_bytes()).unwrap();

        assert!(tokenizer.next_token().unwrap() == Token::OpenBlock);
    }

    #[test]
    fn open_brace_whitespace() {
        let mut tokenizer = Tokenizer::from_io(r#"           {    "#.as_bytes()).unwrap();

        assert!(tokenizer.next_token().unwrap() == Token::OpenBlock);
    }

    #[test]
    fn close_brace() {
        let mut tokenizer = Tokenizer::from_io(r#"}"#.as_bytes()).unwrap();

        assert!(tokenizer.next_token().unwrap() == Token::CloseBlock);
    }

    #[test]
    fn close_brace_whitespace() {
        let mut tokenizer = Tokenizer::from_io(r#"           }    "#.as_bytes()).unwrap();

        assert!(tokenizer.next_token().unwrap() == Token::CloseBlock);
    }

    #[test]
    fn multiple_tokens() {
        let kv_data = r#"
        "str1" { "da\"ta" moredata} } {}"ha {"
        "MULTIPLE LINES!!!!"{}}}you\ kno"w
        "#.as_bytes();


        let mut tokenizer = Tokenizer::from_io(kv_data).unwrap();

        let tokens = vec!(
            Token::Text(r#"str1"#.into()),
            Token::OpenBlock,
            Token::Text(r#"da"ta"#.into()),
            Token::Text(r#"moredata"#.into()),
            Token::CloseBlock,
            Token::CloseBlock,
            Token::OpenBlock,
            Token::CloseBlock,
            Token::Text(r#"ha {"#.into()),
            Token::Text(r#"MULTIPLE LINES!!!!"#.into()),
            Token::OpenBlock,
            Token::CloseBlock,
            Token::CloseBlock,
            Token::CloseBlock,
            Token::Text(r#"you kno"w"#.into()),
            Token::Eof,
        );

        for token in tokens {
            assert!(tokenizer.next_token().unwrap() == token);
        }
    }
}
