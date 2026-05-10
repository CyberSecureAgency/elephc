//! Purpose:
//! Parses PHP list and bracket destructuring assignment patterns.
//! Validates positional, keyed, skipped, and append targets before lowering to ordinary statements.
//!
//! Called from:
//! - `crate::parser::stmt::parse_stmt()` through assignment dispatch.
//!
//! Key details:
//! - Pattern validation rejects malformed destructuring before lowerers synthesize temporary access expressions.

use crate::errors::CompileError;
use crate::lexer::Token;
use crate::parser::ast::{Expr, ExprKind, Stmt};
use crate::parser::expr::{parse_assignment_value_expr, parse_expr};
use crate::span::Span;

use super::super::{expect_semicolon, expect_token};

mod lower;

use lower::lower_list_unpack;

pub(in crate::parser::stmt) fn parse_list_unpack(
    tokens: &[(Token, Span)],
    pos: &mut usize,
    span: Span,
) -> Result<Stmt, CompileError> {
    let pattern = parse_bracket_list_pattern(tokens, pos, span)?;

    expect_token(
        tokens,
        pos,
        &Token::Assign,
        "Expected '=' after list pattern",
    )?;

    let value = parse_assignment_value_expr(tokens, pos)?;
    expect_semicolon(tokens, pos)?;

    Ok(lower_list_unpack(pattern, value, span))
}

pub(in crate::parser::stmt) fn parse_list_construct_unpack(
    tokens: &[(Token, Span)],
    pos: &mut usize,
    span: Span,
) -> Result<Stmt, CompileError> {
    let pattern = parse_list_construct_pattern(tokens, pos, span)?;

    expect_token(
        tokens,
        pos,
        &Token::Assign,
        "Expected '=' after list pattern",
    )?;

    let value = parse_assignment_value_expr(tokens, pos)?;
    expect_semicolon(tokens, pos)?;

    Ok(lower_list_unpack(pattern, value, span))
}

#[derive(Debug, Clone)]
struct ListPattern {
    entries: Vec<ListEntry>,
}

#[derive(Debug, Clone)]
enum ListEntry {
    Skip,
    Target {
        key: Option<Expr>,
        target: ListTarget,
    },
}

#[derive(Debug, Clone)]
enum ListTarget {
    Expr(Expr),
    Append(Expr),
    Nested(ListPattern),
}

fn parse_bracket_list_pattern(
    tokens: &[(Token, Span)],
    pos: &mut usize,
    span: Span,
) -> Result<ListPattern, CompileError> {
    parse_delimited_list_pattern(tokens, pos, span, Token::LBracket, Token::RBracket, "]")
}

fn parse_list_construct_pattern(
    tokens: &[(Token, Span)],
    pos: &mut usize,
    span: Span,
) -> Result<ListPattern, CompileError> {
    *pos += 1; // consume list
    parse_delimited_list_pattern(tokens, pos, span, Token::LParen, Token::RParen, ")")
}

fn parse_delimited_list_pattern(
    tokens: &[(Token, Span)],
    pos: &mut usize,
    span: Span,
    open: Token,
    close: Token,
    close_label: &str,
) -> Result<ListPattern, CompileError> {
    if *pos >= tokens.len() || tokens[*pos].0 != open {
        return Err(CompileError::new(
            span,
            &format!("Expected '{}' after list", open_label(&open)),
        ));
    }
    let close_pos = find_matching_delimiter(tokens, *pos, &open, &close).ok_or_else(|| {
        CompileError::new(
            span,
            &format!("Expected '{}' after list pattern", close_label),
        )
    })?;
    let pattern = parse_list_pattern_content(&tokens[*pos + 1..close_pos], span)?;
    *pos = close_pos + 1;
    Ok(pattern)
}

fn parse_list_pattern_content(
    tokens: &[(Token, Span)],
    span: Span,
) -> Result<ListPattern, CompileError> {
    let mut entries = Vec::new();
    let mut start = 0usize;
    let mut bracket_depth = 0usize;
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;

    for i in 0..tokens.len() {
        let split = matches!(tokens[i].0, Token::Comma)
            && bracket_depth == 0
            && paren_depth == 0
            && brace_depth == 0;

        if split {
            let segment = &tokens[start..i];
            entries.push(parse_list_pattern_segment(segment, span)?);
            start = i + 1;
            continue;
        }

        match tokens[i].0 {
            Token::LBracket => bracket_depth += 1,
            Token::RBracket => bracket_depth = bracket_depth.saturating_sub(1),
            Token::LParen => paren_depth += 1,
            Token::RParen => paren_depth = paren_depth.saturating_sub(1),
            Token::LBrace => brace_depth += 1,
            Token::RBrace => brace_depth = brace_depth.saturating_sub(1),
            _ => {}
        }
    }
    if start < tokens.len() {
        entries.push(parse_list_pattern_segment(&tokens[start..], span)?);
    }

    let pattern = ListPattern { entries };
    validate_list_pattern(&pattern, span)?;
    Ok(pattern)
}

fn parse_list_pattern_segment(
    segment: &[(Token, Span)],
    span: Span,
) -> Result<ListEntry, CompileError> {
    if segment.is_empty() {
        return Ok(ListEntry::Skip);
    }

    if let Some(arrow) = find_top_level_double_arrow(segment) {
        if arrow == 0 {
            return Err(CompileError::new(span, "Expected key before '=>'"));
        }
        if arrow + 1 >= segment.len() {
            return Err(CompileError::new(span, "Expected target after '=>'"));
        }
        let key = parse_expr_from_slice(&segment[..arrow], span)?;
        let target = parse_list_target_from_slice(&segment[arrow + 1..], span)?;
        return Ok(ListEntry::Target {
            key: Some(key),
            target,
        });
    }

    Ok(ListEntry::Target {
        key: None,
        target: parse_list_target_from_slice(segment, span)?,
    })
}

fn parse_list_target_from_slice(
    tokens: &[(Token, Span)],
    span: Span,
) -> Result<ListTarget, CompileError> {
    if tokens.is_empty() {
        return Err(CompileError::new(span, "Expected target in list unpacking"));
    }

    if is_wrapped_by(tokens, 0, Token::LBracket, Token::RBracket) {
        let nested = parse_list_pattern_content(&tokens[1..tokens.len() - 1], span)?;
        return Ok(ListTarget::Nested(nested));
    }

    if is_list_construct_slice(tokens) {
        let nested = parse_list_pattern_content(&tokens[2..tokens.len() - 1], span)?;
        return Ok(ListTarget::Nested(nested));
    }

    if tokens.len() >= 2
        && tokens[tokens.len() - 2].0 == Token::LBracket
        && tokens[tokens.len() - 1].0 == Token::RBracket
    {
        let base = parse_expr_from_slice(&tokens[..tokens.len() - 2], span)?;
        if is_append_target_base(&base) {
            return Ok(ListTarget::Append(base));
        }
        return Err(CompileError::new(span, "Invalid list destructuring target"));
    }

    let expr = parse_expr_from_slice(tokens, span)?;
    if is_list_destructuring_target(&expr) {
        Ok(ListTarget::Expr(expr))
    } else {
        Err(CompileError::new(
            span,
            "Invalid list destructuring target",
        ))
    }
}

fn parse_expr_from_slice(tokens: &[(Token, Span)], span: Span) -> Result<Expr, CompileError> {
    let mut pos = 0usize;
    let expr = parse_expr(tokens, &mut pos)?;
    if pos != tokens.len() {
        return Err(CompileError::new(span, "Unexpected token in list pattern"));
    }
    Ok(expr)
}

fn validate_list_pattern(pattern: &ListPattern, span: Span) -> Result<(), CompileError> {
    if list_pattern_target_count(pattern) == 0 {
        return Err(CompileError::new(span, "Cannot use empty list"));
    }

    let has_keyed = pattern
        .entries
        .iter()
        .any(|entry| matches!(entry, ListEntry::Target { key: Some(_), .. }));
    let has_unkeyed = pattern.entries.iter().any(|entry| {
        matches!(
            entry,
            ListEntry::Skip | ListEntry::Target { key: None, .. }
        )
    });
    if has_keyed && has_unkeyed {
        return Err(CompileError::new(
            span,
            "Cannot mix keyed and unkeyed list entries",
        ));
    }

    Ok(())
}

fn list_pattern_target_count(pattern: &ListPattern) -> usize {
    pattern
        .entries
        .iter()
        .map(|entry| match entry {
            ListEntry::Skip => 0,
            ListEntry::Target {
                target: ListTarget::Nested(pattern),
                ..
            } => list_pattern_target_count(pattern),
            ListEntry::Target { .. } => 1,
        })
        .sum()
}

fn find_top_level_double_arrow(tokens: &[(Token, Span)]) -> Option<usize> {
    let mut bracket_depth = 0usize;
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    for (i, (token, _)) in tokens.iter().enumerate() {
        match token {
            Token::DoubleArrow if bracket_depth == 0 && paren_depth == 0 && brace_depth == 0 => {
                return Some(i);
            }
            Token::LBracket => bracket_depth += 1,
            Token::RBracket => bracket_depth = bracket_depth.saturating_sub(1),
            Token::LParen => paren_depth += 1,
            Token::RParen => paren_depth = paren_depth.saturating_sub(1),
            Token::LBrace => brace_depth += 1,
            Token::RBrace => brace_depth = brace_depth.saturating_sub(1),
            _ => {}
        }
    }
    None
}

fn find_matching_delimiter(
    tokens: &[(Token, Span)],
    open_pos: usize,
    open: &Token,
    close: &Token,
) -> Option<usize> {
    let mut depth = 0usize;
    for (i, (token, _)) in tokens.iter().enumerate().skip(open_pos) {
        if token == open {
            depth += 1;
        } else if token == close {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

fn is_wrapped_by(tokens: &[(Token, Span)], open_pos: usize, open: Token, close: Token) -> bool {
    if tokens.get(open_pos).map(|(token, _)| token) != Some(&open) {
        return false;
    }
    find_matching_delimiter(tokens, open_pos, &open, &close) == Some(tokens.len() - 1)
}

fn is_list_construct_slice(tokens: &[(Token, Span)]) -> bool {
    matches!(
        tokens,
        [(Token::Identifier(name), _), (Token::LParen, _), ..]
            if name.eq_ignore_ascii_case("list")
    ) && is_wrapped_by(tokens, 1, Token::LParen, Token::RParen)
}

fn is_list_destructuring_target(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Variable(_)
        | ExprKind::PropertyAccess { .. }
        | ExprKind::StaticPropertyAccess { .. } => true,
        ExprKind::ArrayAccess { array, .. } => matches!(
            &array.kind,
            ExprKind::Variable(_)
                | ExprKind::PropertyAccess { .. }
                | ExprKind::StaticPropertyAccess { .. }
        ),
        _ => false,
    }
}

fn is_append_target_base(expr: &Expr) -> bool {
    matches!(
        &expr.kind,
        ExprKind::Variable(_)
            | ExprKind::PropertyAccess { .. }
            | ExprKind::StaticPropertyAccess { .. }
    )
}

fn open_label(token: &Token) -> &'static str {
    match token {
        Token::LBracket => "[",
        Token::LParen => "(",
        _ => "",
    }
}
