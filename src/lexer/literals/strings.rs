//! Purpose:
//! Scans PHP string literal forms, including interpolation-aware double strings and heredocs.
//! Emits string tokens plus interpolation boundary tokens that the parser can consume.
//!
//! Called from:
//! - `crate::lexer::scan` through `crate::lexer::literals`.
//!
//! Key details:
//! - Escape and interpolation behavior must preserve PHP-compatible text and source spans.

use super::super::cursor::Cursor;
use super::super::token::Token;
use crate::errors::CompileError;
use crate::span::Span;

/// Scan a double-quoted string with interpolation support.
/// Returns one or more tokens: for `"Hello $name!"` it returns
/// `StringLiteral("Hello ") . Variable("name") . StringLiteral("!")`
/// (with Dot tokens for concatenation).
pub(in crate::lexer) fn scan_double_string_interpolated(
    cursor: &mut Cursor,
) -> Result<Vec<(Token, Span)>, CompileError> {
    let span = cursor.span();
    cursor.advance(); // opening "

    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut has_interpolation = false;

    loop {
        match cursor.peek() {
            Some('"') => {
                cursor.advance();
                break;
            }
            Some('\\') => {
                cursor.advance();
                match cursor.advance() {
                    Some('n') => current.push('\n'),
                    Some('t') => current.push('\t'),
                    Some('\\') => current.push('\\'),
                    Some('"') => current.push('"'),
                    Some('$') => current.push('$'),
                    Some('0') => current.push('\0'),
                    Some(c) => {
                        current.push('\\');
                        current.push(c);
                    }
                    None => return Err(CompileError::new(span, "Unterminated string literal")),
                }
            }
            Some('$') => {
                cursor.advance(); // consume '$'
                let mut name = String::new();
                while let Some(ch) = cursor.peek() {
                    if ch.is_ascii_alphanumeric() || ch == '_' {
                        name.push(ch);
                        cursor.advance();
                    } else {
                        break;
                    }
                }
                if name.is_empty() {
                    current.push('$');
                } else {
                    has_interpolation = true;
                    if !current.is_empty() || tokens.is_empty() {
                        if !tokens.is_empty() {
                            tokens.push((Token::Dot, span));
                        }
                        tokens.push((Token::StringLiteral(std::mem::take(&mut current)), span));
                    }
                    if !tokens.is_empty() && !matches!(tokens.last(), Some((Token::Dot, _))) {
                        tokens.push((Token::Dot, span));
                    }
                    tokens.push((Token::Variable(name), span));
                }
            }
            Some(c) => {
                current.push(c);
                cursor.advance();
            }
            None => return Err(CompileError::new(span, "Unterminated string literal")),
        }
    }

    if !has_interpolation {
        return Ok(vec![(Token::StringLiteral(current), span)]);
    }

    if !current.is_empty() {
        tokens.push((Token::Dot, span));
        tokens.push((Token::StringLiteral(current), span));
    }

    let mut result = vec![(Token::LParen, span)];
    result.extend(tokens);
    result.push((Token::RParen, span));
    Ok(result)
}

pub(in crate::lexer) fn scan_single_string(cursor: &mut Cursor) -> Result<Token, CompileError> {
    let span = cursor.span();
    cursor.advance(); // opening '

    let mut value = String::new();

    loop {
        match cursor.advance() {
            Some('\'') => return Ok(Token::StringLiteral(value)),
            Some('\\') => match cursor.peek() {
                Some('\'') => {
                    cursor.advance();
                    value.push('\'');
                }
                Some('\\') => {
                    cursor.advance();
                    value.push('\\');
                }
                _ => value.push('\\'),
            },
            Some(c) => value.push(c),
            None => return Err(CompileError::new(span, "Unterminated string literal")),
        }
    }
}

/// Scan a heredoc or nowdoc string.
/// At this point, `<<<` has already been consumed.
/// Heredoc: `<<<LABEL` or `<<<"LABEL"` — supports variable interpolation like double-quoted strings
/// Nowdoc: `<<<'LABEL'` — no interpolation (like single-quoted strings)
pub(in crate::lexer) fn scan_heredoc(
    cursor: &mut Cursor,
) -> Result<Vec<(Token, Span)>, CompileError> {
    let span = cursor.span();

    while cursor.peek() == Some(' ') || cursor.peek() == Some('\t') {
        cursor.advance();
    }

    let is_nowdoc = cursor.peek() == Some('\'');
    let is_quoted_heredoc = cursor.peek() == Some('"');

    if is_nowdoc || is_quoted_heredoc {
        cursor.advance();
    }

    let mut label = String::new();
    while let Some(ch) = cursor.peek() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            label.push(ch);
            cursor.advance();
        } else {
            break;
        }
    }

    if label.is_empty() {
        return Err(CompileError::new(span, "Expected heredoc/nowdoc label after '<<<'"));
    }

    if is_nowdoc {
        if cursor.peek() != Some('\'') {
            return Err(CompileError::new(span, "Expected closing ' for nowdoc label"));
        }
        cursor.advance();
    } else if is_quoted_heredoc {
        if cursor.peek() != Some('"') {
            return Err(CompileError::new(span, "Expected closing \" for heredoc label"));
        }
        cursor.advance();
    }

    if cursor.peek() == Some('\r') {
        cursor.advance();
    }
    if cursor.peek() == Some('\n') {
        cursor.advance();
    } else {
        return Err(CompileError::new(span, "Expected newline after heredoc/nowdoc label"));
    }

    let mut content = String::new();
    loop {
        if cursor.is_eof() {
            return Err(CompileError::new(span, "Unterminated heredoc/nowdoc"));
        }

        let remaining = cursor.remaining();

        let mut ws_count = 0;
        for b in remaining.bytes() {
            if b == b' ' || b == b'\t' {
                ws_count += 1;
            } else {
                break;
            }
        }

        let after_ws = &remaining[ws_count..];
        if after_ws.starts_with(&label) {
            let after_label = &after_ws[label.len()..];
            if after_label.is_empty()
                || after_label.starts_with(';')
                || after_label.starts_with('\n')
                || after_label.starts_with('\r')
            {
                for _ in 0..ws_count {
                    cursor.advance();
                }
                for _ in 0..label.len() {
                    cursor.advance();
                }

                if content.ends_with('\n') {
                    content.pop();
                    if content.ends_with('\r') {
                        content.pop();
                    }
                }

                if is_nowdoc {
                    return Ok(vec![(Token::StringLiteral(content), span)]);
                }

                return Ok(interpolate_heredoc_content(&content, span));
            }
        }

        match cursor.advance() {
            Some(ch) => content.push(ch),
            None => return Err(CompileError::new(span, "Unterminated heredoc/nowdoc")),
        }
    }
}

/// Interpolate variables and process escape sequences in heredoc content.
/// Handles both in a single pass so that `\$` produces a literal `$` without triggering
/// variable interpolation. Scans for `$identifier` patterns and expands them into
/// concatenation tokens: `Hello $name!` -> `("Hello " . $name . "!")`
fn interpolate_heredoc_content(content: &str, span: Span) -> Vec<(Token, Span)> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut has_interpolation = false;
    let mut chars = content.chars().peekable();

    loop {
        match chars.peek() {
            None => break,
            Some(&'\\') => {
                chars.next();
                match chars.peek() {
                    Some(&'n') => {
                        chars.next();
                        current.push('\n');
                    }
                    Some(&'t') => {
                        chars.next();
                        current.push('\t');
                    }
                    Some(&'\\') => {
                        chars.next();
                        current.push('\\');
                    }
                    Some(&'"') => {
                        chars.next();
                        current.push('"');
                    }
                    Some(&'$') => {
                        chars.next();
                        current.push('$');
                    }
                    Some(&'0') => {
                        chars.next();
                        current.push('\0');
                    }
                    Some(&c) => {
                        chars.next();
                        current.push('\\');
                        current.push(c);
                    }
                    None => current.push('\\'),
                }
            }
            Some(&'$') => {
                chars.next();
                let mut name = String::new();
                while let Some(&ch) = chars.peek() {
                    if ch.is_ascii_alphanumeric() || ch == '_' {
                        name.push(ch);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if name.is_empty() {
                    current.push('$');
                } else {
                    has_interpolation = true;
                    if !current.is_empty() || tokens.is_empty() {
                        if !tokens.is_empty() {
                            tokens.push((Token::Dot, span));
                        }
                        tokens.push((
                            Token::StringLiteral(std::mem::take(&mut current)),
                            span,
                        ));
                    }
                    if !tokens.is_empty() && !matches!(tokens.last(), Some((Token::Dot, _))) {
                        tokens.push((Token::Dot, span));
                    }
                    tokens.push((Token::Variable(name), span));
                }
            }
            Some(&ch) => {
                current.push(ch);
                chars.next();
            }
        }
    }

    if !has_interpolation {
        return vec![(Token::StringLiteral(current), span)];
    }

    if !current.is_empty() {
        tokens.push((Token::Dot, span));
        tokens.push((Token::StringLiteral(current), span));
    }

    let mut result = vec![(Token::LParen, span)];
    result.extend(tokens);
    result.push((Token::RParen, span));
    result
}
