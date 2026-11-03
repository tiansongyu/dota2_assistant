//! A bounded KV1 subset. Unknown values are consumed, never retained in the result.
use crate::Candidate;
use std::collections::BTreeSet;

pub const MAX_FILE_BYTES: usize = 1024 * 1024;
const MAX_TOKEN_BYTES: usize = 4096;
const MAX_DEPTH: usize = 16;
const MAX_NODES: usize = 10_000;
const MAX_USERS: usize = 256;

#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    #[error("VDF 文件超过大小限制")]
    TooLarge,
    #[error("VDF 不是有效 UTF-8 文本")]
    Encoding,
    #[error("VDF 格式无效或包含不支持的指令")]
    Syntax,
    #[error("VDF 包含重复键，未选择任意一个值")]
    DuplicateKey,
    #[error("VDF 超过层数、字段数或字符串长度限制")]
    Limit,
    #[error("VDF 账号或所需字段格式无效")]
    InvalidField,
}

enum Token {
    Text(String),
    Open,
    Close,
    End,
}

struct Parser<'a> {
    text: &'a str,
    offset: usize,
    nodes: usize,
}

impl Parser<'_> {
    fn next(&mut self) -> Result<Token, ParseError> {
        let bytes = self.text.as_bytes();
        loop {
            while bytes.get(self.offset).is_some_and(u8::is_ascii_whitespace) {
                self.offset += 1;
            }
            if self.text[self.offset..].starts_with("//") {
                self.offset += self.text[self.offset..]
                    .find('\n')
                    .unwrap_or(bytes.len() - self.offset);
            } else if self.text[self.offset..].starts_with("/*") {
                let end = self.text[self.offset + 2..]
                    .find("*/")
                    .ok_or(ParseError::Syntax)?;
                self.offset += end + 4;
            } else {
                break;
            }
        }
        let Some(&byte) = bytes.get(self.offset) else {
            return Ok(Token::End);
        };
        self.offset += 1;
        match byte {
            b'{' => Ok(Token::Open),
            b'}' => Ok(Token::Close),
            b'"' => {
                let mut value = String::new();
                loop {
                    let ch = self.text[self.offset..]
                        .chars()
                        .next()
                        .ok_or(ParseError::Syntax)?;
                    self.offset += ch.len_utf8();
                    if ch == '"' {
                        return Ok(Token::Text(value));
                    }
                    if ch == '\\' {
                        let escaped = self.text[self.offset..]
                            .chars()
                            .next()
                            .ok_or(ParseError::Syntax)?;
                        self.offset += escaped.len_utf8();
                        match escaped {
                            '"' | '\\' => value.push(escaped),
                            'n' => value.push('\n'),
                            'r' => value.push('\r'),
                            't' => value.push('\t'),
                            _ => {
                                value.push('\\');
                                value.push(escaped);
                            }
                        }
                    } else if ch == '\0' {
                        return Err(ParseError::Syntax);
                    } else {
                        value.push(ch);
                    }
                    if value.len() > MAX_TOKEN_BYTES {
                        return Err(ParseError::Limit);
                    }
                }
            }
            b'#' | b'[' | b']' | 0 => Err(ParseError::Syntax),
            _ => {
                let start = self.offset - 1;
                while bytes
                    .get(self.offset)
                    .is_some_and(|b| !b.is_ascii_whitespace() && !b"{}\"".contains(b))
                {
                    self.offset += 1;
                    if self.offset - start > MAX_TOKEN_BYTES {
                        return Err(ParseError::Limit);
                    }
                }
                let value = &self.text[start..self.offset];
                if value.contains(['\0', '[', ']', '#']) {
                    return Err(ParseError::Syntax);
                }
                Ok(Token::Text(value.to_owned()))
            }
        }
    }

    fn key(
        &mut self,
        token: Token,
        seen: &mut BTreeSet<String>,
    ) -> Result<Option<String>, ParseError> {
        match token {
            Token::Close => Ok(None),
            Token::Text(key) => {
                self.nodes += 1;
                if self.nodes > MAX_NODES {
                    return Err(ParseError::Limit);
                }
                if !seen.insert(key.to_ascii_lowercase()) {
                    return Err(ParseError::DuplicateKey);
                }
                Ok(Some(key))
            }
            _ => Err(ParseError::Syntax),
        }
    }

    fn skip_object(&mut self, depth: usize) -> Result<(), ParseError> {
        if depth > MAX_DEPTH {
            return Err(ParseError::Limit);
        }
        let mut seen = BTreeSet::new();
        loop {
            let token = self.next()?;
            if self.key(token, &mut seen)?.is_none() {
                return Ok(());
            }
            match self.next()? {
                Token::Text(_) => (),
                Token::Open => self.skip_object(depth + 1)?,
                _ => return Err(ParseError::Syntax),
            }
        }
    }

    fn user(&mut self, steam64: String) -> Result<Candidate, ParseError> {
        let account_id =
            dpda_domain::normalize_account_id(&steam64).map_err(|_| ParseError::InvalidField)?;
        if dpda_domain::steam64(account_id) != steam64 {
            return Err(ParseError::InvalidField);
        }
        let mut result = Candidate {
            account_id,
            steam64,
            display_name: None,
            most_recent: false,
            last_used_at: None,
        };
        let mut seen = BTreeSet::new();
        loop {
            let token = self.next()?;
            let Some(key) = self.key(token, &mut seen)? else {
                return Ok(result);
            };
            let field = key.to_ascii_lowercase();
            match self.next()? {
                Token::Text(value) => match field.as_str() {
                    "personaname" => {
                        let clean: String = value
                            .chars()
                            .filter(|ch| !ch.is_control())
                            .take(256)
                            .collect();
                        let clean = clean.trim();
                        result.display_name = (!clean.is_empty()).then(|| clean.to_owned());
                    }
                    "mostrecent" => {
                        result.most_recent = match value.as_str() {
                            "0" => false,
                            "1" => true,
                            _ => return Err(ParseError::InvalidField),
                        }
                    }
                    "timestamp" => {
                        let value = value.parse::<i64>().map_err(|_| ParseError::InvalidField)?;
                        // Year 9999 is also safely representable by JavaScript integers.
                        if !(0..=253_402_300_799).contains(&value) {
                            return Err(ParseError::InvalidField);
                        }
                        result.last_used_at = (value > 0).then_some(value);
                    }
                    _ => (),
                },
                Token::Open
                    if !matches!(field.as_str(), "personaname" | "mostrecent" | "timestamp") =>
                {
                    self.skip_object(3)?
                }
                _ => return Err(ParseError::InvalidField),
            }
        }
    }
}

/// Parse only local account hints. No include files, authentication or network I/O.
pub fn parse_loginusers(bytes: &[u8]) -> Result<Vec<Candidate>, ParseError> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err(ParseError::TooLarge);
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| ParseError::Encoding)?
        .trim_start_matches('\u{feff}');
    let mut parser = Parser {
        text,
        offset: 0,
        nodes: 0,
    };
    if !matches!(parser.next()?, Token::Text(key) if key.eq_ignore_ascii_case("users"))
        || !matches!(parser.next()?, Token::Open)
    {
        return Err(ParseError::Syntax);
    }
    let mut seen = BTreeSet::new();
    let mut users = Vec::new();
    loop {
        let token = parser.next()?;
        let Some(key) = parser.key(token, &mut seen)? else {
            break;
        };
        if users.len() >= MAX_USERS {
            return Err(ParseError::Limit);
        }
        if !matches!(parser.next()?, Token::Open) {
            return Err(ParseError::Syntax);
        }
        users.push(parser.user(key)?);
    }
    if !matches!(parser.next()?, Token::End) {
        return Err(ParseError::Syntax);
    }
    Ok(users)
}
