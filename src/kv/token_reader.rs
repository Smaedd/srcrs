use std::io::{Read, Result};

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
                        new_string.push(new_peek);
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
                        self.consume_comment()?;
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
