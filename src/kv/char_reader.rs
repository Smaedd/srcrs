use std::io::{Error, ErrorKind, Read, Result};

const READ_SIZE: usize = 1024;
const ESCAPE: char = '\\';
const COMMENT: char = '/';
const QUOTE: char = '"';

#[derive(Debug, PartialEq, Clone)]
pub enum ReadChar {
    Normal(char),
    Escaped(char),
    Whitespace,
    Eof,
}

impl ReadChar {
    #[inline]
    pub fn is_normal(&self) -> bool {
        matches!(self, Self::Normal(_))
    }

    #[inline]
    pub fn unwrap_normal(&self) -> char {
        match self {
            Self::Normal(data) => *data,
            _ => panic!("called ReadChar::unwrap_normal() on {:?}", self),
        }
    }

    #[inline]
    pub fn is_escaped(&self) -> bool {
        matches!(self, Self::Escaped(_))
    }

    #[inline]
    pub fn unwrap_escaped(&self) -> char {
        match self {
            Self::Escaped(data) => *data,
            _ => panic!("called ReadChar::unwrap_escaped() on {:?}", self),
        }
    }

    #[inline]
    pub fn is_whitespace(&self) -> bool {
        matches!(self, Self::Whitespace)
    }

    #[inline]
    pub fn unwrap_whitespace(&self) -> () {
        match self {
            Self::Whitespace => (),
            _ => panic!("called ReadChar::unwrap_whitespace() on {:?}", self),
        }
    }

    #[inline]
    pub fn is_eof(&self) -> bool {
        matches!(self, Self::Eof)
    }

    #[inline]
    pub fn unwrap_eof(&self) -> () {
        match self {
            Self::Eof => (),
            _ => panic!("called ReadChar::unwrap_eof() on {:?}", self),
        }
    }

    #[inline]
    pub fn is_char(&self) -> bool {
        self.is_normal() || self.is_escaped()
    }

    #[inline]
    pub fn unwrap_char(&self) -> char {
        match self {
            Self::Normal(data) => *data,
            Self::Escaped(data) => *data,
            _ => panic!("called ReadChar::unwrap_char() on {:?}", self),
        }
    }
}

pub struct CharReader<R>
where
    R: Read,
{
    reader: R,

    last_read: [u8; READ_SIZE],
    last_token: ReadChar,
    position: usize,
    is_quoted: bool,
    max_read: usize,

    num_read: u64,
}

impl<R: Read> CharReader<R> {
    pub fn from_io(mut read: R) -> Result<Self> {
        let mut last_read = [0u8; READ_SIZE];
        let max_read: usize = read.read(&mut last_read)?;

        let mut new_self = Self {
            reader: read,

            last_read: last_read,
            last_token: ReadChar::Whitespace,
            position: 0,
            is_quoted: false,
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

    pub fn advance(&mut self) -> Result<()> {
        // Horrible but needs to be done like this
        if self.peek() == ReadChar::Whitespace {
            self.advance_internal()?;
            while self.peek() == ReadChar::Whitespace {
                self.advance_internal()?;
            }
        } else {
            self.advance_internal()?;
        }

        Ok(())
    }

    #[inline]
    fn advance_internal(&mut self) -> Result<()> {
        let old_peek = self.peek_char();
        self.advance_char()?;

        match old_peek {
            None => self.last_token = ReadChar::Eof,
            Some(data) => match data {
                ESCAPE => {
                    let next_read = self.peek_char().ok_or_else(|| self.invalid_char())?;
                    self.advance_char()?;

                    self.last_token = ReadChar::Escaped(next_read); // This means that comments get escaped. I'm fine with this
                }
                COMMENT => {
                    if self.is_quoted {
                        self.last_token = ReadChar::Normal(data);
                    } else {
                        match self.peek_char() {
                            None => self.last_token = ReadChar::Normal(data),
                            Some(next_data) => match next_data {
                                COMMENT => {
                                    self.consume_comment()?;
                                    self.last_token = ReadChar::Whitespace;
                                }
                                _ => self.last_token = ReadChar::Normal(data),
                            },
                        }
                    }
                }
                _ => {
                    if data == QUOTE {
                        // We want to preserve whitespace when quoted, so we have to do a bunch of garbage
                        self.is_quoted = !self.is_quoted;
                        self.last_token = ReadChar::Normal(data);
                    } else if self.is_quoted {
                        self.last_token = ReadChar::Normal(data);
                    } else if data.is_whitespace() {
                        self.last_token = ReadChar::Whitespace;
                    } else {
                        self.last_token = ReadChar::Normal(data);
                    }
                }
            },
        }

        Ok(())
    }

    #[inline]
    pub fn peek(&self) -> ReadChar {
        self.last_token.clone()
    }

    fn advance_char(&mut self) -> Result<()> {
        self.position += 1;
        self.num_read += 1;

        if self.position >= self.max_read {
            self.max_read = self.reader.read(&mut self.last_read)?;
            self.position = 0;
        }

        Ok(())
    }

    fn peek_char(&self) -> Option<char> {
        if self.max_read == 0 {
            return None;
        }

        return Some(self.last_read[self.position] as char);
    }

    fn consume_comment(&mut self) -> Result<()> {
        while let Some(data) = self.peek_char() {
            self.advance_char()?;

            if data == '\n' {
                break;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::{CharReader, ReadChar};

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
            ReadChar::Normal('B'), 
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
            ReadChar::Escaped('b'), 
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
            ReadChar::Normal('b'), 
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
