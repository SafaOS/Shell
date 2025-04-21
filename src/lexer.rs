use std::{borrow::Cow, str::CharIndices};

pub struct Lexer<'a> {
    input_raw: &'a str,
    chars: CharIndices<'a>,
}

#[derive(Debug, Clone, Copy)]
pub enum Token<'a> {
    Word(&'a str),
    Str(&'a str),
    Var(&'a str),
}

impl<'a> Token<'a> {
    pub fn as_str(&self) -> Cow<'a, str> {
        match self {
            Token::Word(str) | Token::Str(str) => Cow::Borrowed(str),
            Token::Var(name) => Cow::Owned(std::env::var(name).unwrap_or_default()),
        }
    }
}

impl<'a> Iterator for Lexer<'a> {
    type Item = Token<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next()
    }
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            input_raw: input,
            chars: input.char_indices(),
        }
    }

    pub fn next(&mut self) -> Option<Token<'a>> {
        match self.chars.next()? {
            (_, c) if c.is_whitespace() => self.next(),
            (start, quote) if quote == '"' || quote == '\'' => {
                let start = start + 1;
                let mut end = start;

                while !self.chars.next().is_none_or(|(_, c)| c == quote) {
                    end += 1;
                }

                Some(Token::Str(&self.input_raw[start..end]))
            }
            (start, '$') => {
                let start = start + 1;
                let mut end = start;

                while !self.chars.next().is_none_or(|(_, c)| c.is_whitespace()) {
                    end += 1;
                }

                Some(Token::Var(&self.input_raw[start..end]))
            }
            (start, _) => {
                let mut end = start;
                while !self.chars.next().is_none_or(|(_, c)| c.is_whitespace()) {
                    end += 1;
                }

                Some(Token::Word(&self.input_raw[start..end + 1]))
            }
        }
    }
}
