//! Purpose:
//! Scans integer and floating-point literals across PHP-supported numeric forms.
//! Handles radix prefixes, underscores, decimal/exponent floats, and dot-started floats.
//!
//! Called from:
//! - `crate::lexer::scan` through `crate::lexer::literals`.
//!
//! Key details:
//! - Invalid trailing alphanumerics become lexer errors instead of being split into misleading tokens.

use super::super::cursor::Cursor;
use super::super::token::Token;
use crate::errors::CompileError;

/// Collect digits according to `is_digit`, allowing a single `_` between digits
/// (PHP 7.4+ numeric separator). The helper never consumes a leading or trailing
/// `_` — those remain on the cursor so [`validate_no_trailing_alnum`] can flag
/// them. Returns the digit string with separators stripped.
fn scan_radix_digits<F: Fn(char) -> bool>(cursor: &mut Cursor, is_digit: F) -> String {
    let mut s = String::new();
    while let Some(ch) = cursor.peek() {
        if is_digit(ch) {
            s.push(ch);
            cursor.advance();
        } else if ch == '_' && !s.is_empty() {
            let remaining = cursor.remaining();
            let next_is_digit =
                remaining.len() > 1 && is_digit(remaining.as_bytes()[1] as char);
            if next_is_digit {
                cursor.advance();
            } else {
                break;
            }
        } else {
            break;
        }
    }
    s
}

/// After scanning a numeric literal, ensure no alphanumeric character or `_`
/// follows. Catches malformed forms like `0o78`, `078`, `0xfg`, `0b12`, `1_`,
/// and `1__0`, which PHP rejects at parse time but the lexer would otherwise
/// silently split into two adjacent tokens.
fn validate_no_trailing_alnum(cursor: &Cursor, base_label: &str) -> Result<(), CompileError> {
    if let Some(ch) = cursor.peek() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            return Err(CompileError::new(
                cursor.span(),
                &format!("Unexpected character '{ch}' after {base_label} literal"),
            ));
        }
    }
    Ok(())
}

pub(in crate::lexer) fn scan_number(cursor: &mut Cursor) -> Result<Token, CompileError> {
    if cursor.peek() == Some('0') {
        let remaining = cursor.remaining();
        if remaining.len() > 1 {
            let prefix = remaining.as_bytes()[1];

            if prefix == b'x' || prefix == b'X' {
                cursor.advance();
                cursor.advance();
                let hex_str = scan_radix_digits(cursor, |c| c.is_ascii_hexdigit());
                if hex_str.is_empty() {
                    return Err(CompileError::new(
                        cursor.span(),
                        "Expected hex digits after '0x'",
                    ));
                }
                validate_no_trailing_alnum(cursor, "hex")?;
                let value = i64::from_str_radix(&hex_str, 16)
                    .map_err(|_| CompileError::new(cursor.span(), "Invalid hex literal"))?;
                return Ok(Token::IntLiteral(value));
            }

            if prefix == b'o' || prefix == b'O' {
                cursor.advance();
                cursor.advance();
                let octal_str = scan_radix_digits(cursor, |c| c.is_ascii_digit() && c < '8');
                if octal_str.is_empty() {
                    return Err(CompileError::new(
                        cursor.span(),
                        "Expected octal digits after '0o'",
                    ));
                }
                validate_no_trailing_alnum(cursor, "octal")?;
                let value = i64::from_str_radix(&octal_str, 8)
                    .map_err(|_| CompileError::new(cursor.span(), "Invalid octal literal"))?;
                return Ok(Token::IntLiteral(value));
            }

            if prefix == b'b' || prefix == b'B' {
                cursor.advance();
                cursor.advance();
                let bin_str = scan_radix_digits(cursor, |c| c == '0' || c == '1');
                if bin_str.is_empty() {
                    return Err(CompileError::new(
                        cursor.span(),
                        "Expected binary digits after '0b'",
                    ));
                }
                validate_no_trailing_alnum(cursor, "binary")?;
                let value = i64::from_str_radix(&bin_str, 2)
                    .map_err(|_| CompileError::new(cursor.span(), "Invalid binary literal"))?;
                return Ok(Token::IntLiteral(value));
            }
        }
    }

    let mut num_str = scan_radix_digits(cursor, |c| c.is_ascii_digit());

    let is_float = if cursor.peek() == Some('.') {
        let remaining = cursor.remaining();
        remaining.len() > 1 && (remaining.as_bytes()[1] as char).is_ascii_digit()
    } else {
        false
    };

    let is_sci = matches!(cursor.peek(), Some('e') | Some('E'));

    if is_float || is_sci {
        if is_float {
            num_str.push('.');
            cursor.advance();
            num_str.push_str(&scan_radix_digits(cursor, |c| c.is_ascii_digit()));
        }
        if matches!(cursor.peek(), Some('e') | Some('E')) {
            num_str.push('e');
            cursor.advance();
            if let Some(sign @ ('+' | '-')) = cursor.peek() {
                num_str.push(sign);
                cursor.advance();
            }
            num_str.push_str(&scan_radix_digits(cursor, |c| c.is_ascii_digit()));
        }
        validate_no_trailing_alnum(cursor, "float")?;
        let value: f64 = num_str
            .parse()
            .map_err(|_| CompileError::new(cursor.span(), "Invalid float literal"))?;
        return Ok(Token::FloatLiteral(value));
    }

    let is_legacy_octal = num_str.len() > 1 && num_str.starts_with('0');
    validate_no_trailing_alnum(
        cursor,
        if is_legacy_octal { "octal" } else { "decimal" },
    )?;
    if is_legacy_octal {
        let value = i64::from_str_radix(&num_str, 8)
            .map_err(|_| CompileError::new(cursor.span(), "Invalid octal literal"))?;
        return Ok(Token::IntLiteral(value));
    }

    let value: i64 = num_str
        .parse()
        .map_err(|_| CompileError::new(cursor.span(), "Invalid integer literal"))?;

    Ok(Token::IntLiteral(value))
}

/// Scan a float literal starting with `.` (e.g., `.5`, `.123`)
pub(in crate::lexer) fn scan_dot_float(cursor: &mut Cursor) -> Result<Token, CompileError> {
    let mut num_str = String::from("0.");
    cursor.advance();

    num_str.push_str(&scan_radix_digits(cursor, |c| c.is_ascii_digit()));

    if matches!(cursor.peek(), Some('e') | Some('E')) {
        num_str.push('e');
        cursor.advance();
        if let Some(sign @ ('+' | '-')) = cursor.peek() {
            num_str.push(sign);
            cursor.advance();
        }
        num_str.push_str(&scan_radix_digits(cursor, |c| c.is_ascii_digit()));
    }

    validate_no_trailing_alnum(cursor, "float")?;

    let value: f64 = num_str
        .parse()
        .map_err(|_| CompileError::new(cursor.span(), "Invalid float literal"))?;

    Ok(Token::FloatLiteral(value))
}
