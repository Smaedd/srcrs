use std::io::{Error, ErrorKind, Read, Result};

use bumpalo::collections::String;
use bumpalo::Bump;
use std::mem;

const REWIND_SIZE: usize = 1;
const READ_SIZE: usize = 1024;

#[derive(Debug, PartialEq, Clone)]
pub enum Token<'a> {
    Text(String<'a>),
    OpenBlock,
    CloseBlock,
    OpenFlag,
    CloseFlag,
    Negate,
    Eof,
}

impl<'a> Token<'a> {
    #[inline]
    pub fn unwrap_text(&mut self) -> String<'a> {
        match self {
            Self::Text(data) => mem::replace(data, String::new_in(data.bump())),
            _ => panic!("called Token::unwrap_text() on {:?}", self),
        }
    }
}

pub struct TokenReader<'a, R>
where
    R: Read,
{
    reader: R,
    allocator: &'a Bump,

    last_read: [u8; READ_SIZE + REWIND_SIZE],
    last_token: Token<'a>,
    position: usize,
    max_read: usize,

    num_read: u64,
}

const BASE_STRING_SIZE: usize = 1024;
const QUOTE: char = '"';
const ESCAPE: char = '\\';
const COMMENT: char = '/';
const OPEN_BLOCK: char = '{';
const CLOSE_BLOCK: char = '}';
const OPEN_FLAG: char = '[';
const CLOSE_FLAG: char = ']';
const NEGATE: char = '!';

impl<'a, R: Read> TokenReader<'a, R> {
    pub fn from_io(mut read: R, allocator: &'a Bump) -> Result<Self> {
        let mut last_read = [0u8; READ_SIZE + REWIND_SIZE];
        let max_read: usize = read.read(&mut last_read[REWIND_SIZE..])? + REWIND_SIZE;

        let mut new_self = Self {
            reader: read,
            allocator: allocator,

            last_read: last_read,
            last_token: Token::Eof,
            position: REWIND_SIZE,
            max_read: max_read,

            num_read: 0,
        };

        // Initialise last_token, reading until there is no whitespace
        new_self.advance()?;

        Ok(new_self)
    }

    #[inline]
    fn invalid_char(&self) -> Error {
        Error::new(
            ErrorKind::InvalidData,
            format!("Invalid char at position {}", self.num_read),
        )
    }

    fn invalid_eof() -> Error {
        Error::new(ErrorKind::InvalidData, stringify!("Unexpected EOF"))
    }

    #[inline]
    pub fn peek(&mut self) -> &mut Token<'a> {
        &mut self.last_token
    }

    pub fn advance(&mut self) -> Result<()> {
        loop {
            match self.peek_char() {
                None => self.last_token = Token::Eof,
                Some(ch) => match ch {
                    OPEN_BLOCK => {
                        self.last_token = Token::OpenBlock;
                        self.advance_char()?;
                    }
                    CLOSE_BLOCK => {
                        self.last_token = Token::CloseBlock;
                        self.advance_char()?;
                    }
                    OPEN_FLAG => {
                        self.last_token = Token::OpenFlag;
                        self.advance_char()?;
                    }
                    CLOSE_FLAG => {
                        self.last_token = Token::CloseFlag;
                        self.advance_char()?;
                    }
                    NEGATE => {
                        self.last_token = Token::Negate;
                        self.advance_char()?;
                    }
                    _ => {
                        if ch.is_whitespace() {
                            self.consume_whitespace()?;
                            continue;
                        }

                        if ch == COMMENT {
                            self.advance_char()?;

                            let new_peek = self.peek_char();
                            if new_peek.is_none() {
                                let mut new_string = String::with_capacity_in(1, self.allocator);
                                new_string.push(ch);
                                self.last_token = Token::Text(new_string);
                                break;
                            } else if new_peek.unwrap() != COMMENT {
                                self.rewind_char(new_peek.unwrap());
                                continue;
                            } else {
                                // Properly formed comment
                                self.consume_comment()?;
                                continue;
                            }
                        }

                        if ch == QUOTE {
                            self.last_token = Token::Text(self.read_quoted_text()?);
                        } else {
                            self.last_token = Token::Text(self.read_unquoted_text()?);
                        }
                    }
                },
            }

            break;
        }

        Ok(())
    }

    #[inline]
    fn consume_comment(&mut self) -> Result<()> {
        // Assumes peek_char() gives us the second /.
        self.advance_char()?;

        while let Some(data) = self.peek_char() {
            self.advance_char()?;

            if data == '\n' {
                break;
            }
        }

        Ok(())
    }

    #[inline]
    fn consume_whitespace(&mut self) -> Result<()> {
        self.advance_char()?;

        while let Some(data) = self.peek_char() {
            if !data.is_whitespace() {
                break;
            }

            self.advance_char()?;
        }

        Ok(())
    }

    fn read_quoted_text(&mut self) -> Result<String<'a>> {
        self.advance_char()?;
        let mut new_string = String::with_capacity_in(BASE_STRING_SIZE, self.allocator);

        while let Some(data) = self.peek_char() {
            self.advance_char()?;

            if data == '"' {
                break;
            }

            new_string.push(data);
        }

        new_string.shrink_to_fit();

        Ok(new_string)
    }

    fn read_unquoted_text(&mut self) -> Result<String<'a>> {
        let mut new_string = String::with_capacity_in(BASE_STRING_SIZE, self.allocator);

        while let Some(data) = self.peek_char() {
            match data {
                OPEN_BLOCK | CLOSE_BLOCK | OPEN_FLAG | CLOSE_FLAG | NEGATE => break,
                _ => {
                    if data.is_whitespace() {
                        break;
                    }
                }
            }

            self.advance_char()?;

            if data == ESCAPE {
                match self.peek_char() {
                    None => {
                        new_string.push(ESCAPE);
                        break;
                    }
                    Some(new_peek) => {
                        new_string.push(self.peek_char().unwrap());
                        self.advance_char()?;
                    }
                }
            }

            if data == COMMENT {
                match self.peek_char() {
                    None => {
                        new_string.push(COMMENT);
                        break;
                    }
                    Some(COMMENT) => {
                        self.consume_comment();
                        break;
                    }
                    _ => {}
                }
            }

            new_string.push(data);
        }

        new_string.shrink_to_fit();
        Ok(new_string)
    }

    fn rewind_char(&mut self, rewind: char) {
        self.last_read[self.position] = rewind as u8;
        self.position -= 1;
    }

    fn advance_char(&mut self) -> Result<()> {
        self.position += 1;
        self.num_read += 1;

        if self.position >= self.max_read {
            self.max_read = self.reader.read(&mut self.last_read[REWIND_SIZE..])? + REWIND_SIZE;
            self.position = REWIND_SIZE;
        }

        Ok(())
    }

    fn peek_char(&self) -> Option<char> {
        if self.max_read == REWIND_SIZE {
            return None;
        }

        return Some(self.last_read[self.position] as char);
    }
}

/*#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::{Token, TokenReader};

    #[test]
    fn empty_input() {
        let tokenizer = CharReader::from_io(r#""#.as_bytes()).unwrap();

        assert!(tokenizer.peek() == ReadChar::Eof);
    }

    fn expect_vec<R>(tokenizer: &mut CharReader<R>, expected: &Vec<ReadChar>)
    where
        R: Read,
    {
        for expected in expected {
            let peeked = tokenizer.peek();
            if peeked != *expected {
                panic!("Expected: {:?}, got {:?}", expected, peeked);
            }

            match tokenizer.advance() {
                Err(err) => {
                    panic!("During processing of {:?}, got error:\n\t{}", expected, err);
                }
                Ok(_) => {}
            }
        }
    }

    #[test]
    fn single_char() {
        let mut tokenizer = CharReader::from_io(r#"a"#.as_bytes()).unwrap();

        #[rustfmt::skip]
        let expected_readchars = vec![
            ReadChar::Normal('a'),
            ReadChar::Eof
        ];

        expect_vec(&mut tokenizer, &expected_readchars);
    }

    #[test]
    fn multiple_chars() {
        let mut tokenizer = CharReader::from_io(r#"aBc"#.as_bytes()).unwrap();

        #[rustfmt::skip]
        let expected_readchars = vec![
            ReadChar::Normal('a'),
            ReadChar::Normal('a'),
            ReadChar::Normal('c'),
            ReadChar::Eof
        ];

        expect_vec(&mut tokenizer, &expected_readchars);
    }

    #[test]
    fn strange_chars() {
        let mut tokenizer = CharReader::from_io(r#"[{;A><*^"#.as_bytes()).unwrap();

        #[rustfmt::skip]
        let expected_readchars = vec![
            ReadChar::Normal('['),
            ReadChar::Normal('{'),
            ReadChar::Normal(';'),
            ReadChar::Normal('A'),
            ReadChar::Normal('>'),
            ReadChar::Normal('<'),
            ReadChar::Normal('*'),
            ReadChar::Normal('^'),
            ReadChar::Eof
        ];

        expect_vec(&mut tokenizer, &expected_readchars);
    }

    #[test]
    fn escaped_single() {
        let mut tokenizer = CharReader::from_io(r#"\a"#.as_bytes()).unwrap();

        #[rustfmt::skip]
        let expected_readchars = vec![
            ReadChar::Escaped('a'),
            ReadChar::Eof
        ];

        expect_vec(&mut tokenizer, &expected_readchars);
    }

    #[test]
    fn escaped_multiple() {
        let mut tokenizer = CharReader::from_io(r#"\a\b\c"#.as_bytes()).unwrap();

        #[rustfmt::skip]
        let expected_readchars = vec![
            ReadChar::Escaped('a'),
            ReadChar::Escaped('a'),
            ReadChar::Escaped('c'),
            ReadChar::Eof
        ];

        expect_vec(&mut tokenizer, &expected_readchars);
    }

    #[test]
    fn escaped_unescaped() {
        let mut tokenizer = CharReader::from_io(r#"\ab\c"#.as_bytes()).unwrap();

        #[rustfmt::skip]
        let expected_readchars = vec![
            ReadChar::Escaped('a'),
            ReadChar::Normal('a'),
            ReadChar::Escaped('c'),
            ReadChar::Eof
        ];

        expect_vec(&mut tokenizer, &expected_readchars);
    }

    #[test]
    fn escaped_unescaped_strange() {
        let mut tokenizer = CharReader::from_io(r#"[\{\;\"A>\<*\^"#.as_bytes()).unwrap();

        #[rustfmt::skip]
        let expected_readchars = vec![
            ReadChar::Normal('['),
            ReadChar::Escaped('{'),
            ReadChar::Escaped(';'),
            ReadChar::Escaped('"'),
            ReadChar::Normal('A'),
            ReadChar::Normal('>'),
            ReadChar::Escaped('<'),
            ReadChar::Normal('*'),
            ReadChar::Escaped('^'),
            ReadChar::Eof
        ];

        expect_vec(&mut tokenizer, &expected_readchars);
    }

    #[test]
    fn single_whitespace_single_char() {
        let mut tokenizer = CharReader::from_io(r#" "#.as_bytes()).unwrap();

        #[rustfmt::skip]
        let expected_readchars = vec![
            ReadChar::Eof
        ];

        expect_vec(&mut tokenizer, &expected_readchars);
    }

    #[test]
    fn single_whitespace_multi_char() {
        let mut tokenizer = CharReader::from_io("\t \n\t ".as_bytes()).unwrap();

        #[rustfmt::skip]
        let expected_readchars = vec![
            ReadChar::Eof
        ];

        expect_vec(&mut tokenizer, &expected_readchars);
    }

    #[test]
    fn whitespace_escaped_unescaped() {
        let mut tokenizer =
            CharReader::from_io("\the\\y \nw\\hat\\'s \\u\\p\t ".as_bytes()).unwrap();

        #[rustfmt::skip]
        let expected_readchars = vec![
            ReadChar::Normal('h'),
            ReadChar::Normal('e'),
            ReadChar::Escaped('y'),
            ReadChar::Whitespace,
            ReadChar::Normal('w'),
            ReadChar::Escaped('h'),
            ReadChar::Normal('a'),
            ReadChar::Normal('t'),
            ReadChar::Escaped('\''),
            ReadChar::Normal('s'),
            ReadChar::Whitespace,
            ReadChar::Escaped('u'),
            ReadChar::Escaped('p'),
            ReadChar::Whitespace,
            ReadChar::Eof
        ];

        expect_vec(&mut tokenizer, &expected_readchars);
    }

    #[test]
    fn single_comment() {
        let mut tokenizer = CharReader::from_io(r#"// Hello"#.as_bytes()).unwrap();

        #[rustfmt::skip]
        let expected_readchars = vec![
            ReadChar::Eof
        ];

        expect_vec(&mut tokenizer, &expected_readchars);
    }

    #[test]
    fn non_comment_slash() {
        let mut tokenizer = CharReader::from_io(r#"/ / Hello"#.as_bytes()).unwrap();

        #[rustfmt::skip]
        let expected_readchars = vec![
            ReadChar::Normal('/'),
            ReadChar::Whitespace,
            ReadChar::Normal('/'),
            ReadChar::Whitespace,
            ReadChar::Normal('H'),
            ReadChar::Normal('e'),
            ReadChar::Normal('l'),
            ReadChar::Normal('l'),
            ReadChar::Normal('o'),
            ReadChar::Eof
        ];

        expect_vec(&mut tokenizer, &expected_readchars);
    }

    #[test]
    fn comments_interweaved() {
        let mut tokenizer = CharReader::from_io(
            r#"
        // I am a comment. Don't mind me!
        h\i// Don't mind me either
        hey
        "#
            .as_bytes(),
        )
        .unwrap();

        #[rustfmt::skip]
        let expected_readchars = vec![
            ReadChar::Normal('h'),
            ReadChar::Escaped('i'),
            ReadChar::Whitespace,
            ReadChar::Normal('h'),
            ReadChar::Normal('e'),
            ReadChar::Normal('y'),
            ReadChar::Whitespace,
            ReadChar::Eof
        ];

        expect_vec(&mut tokenizer, &expected_readchars);
    }
}
*/
