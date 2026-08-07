//! The course DSL's **lexer**: source text to a flat token list, with a line
//! and column on every token.
//!
//! It is deliberately tiny. The grammar has no expressions, no operators, no
//! string escapes beyond the obvious, and no nesting rules the lexer needs to
//! know about — so the whole thing is one pass over the characters with a
//! four-way branch on what starts a token. Nothing here can execute anything;
//! the most complicated thing it produces is a number with a unit suffix.
//!
//! Every token carries where it came from, because a diagnostic that cannot name
//! a line is not much of a diagnostic.

use crate::course::error::{CourseError, CourseErrorCode, CourseResult, SourceLocation};
use crate::course::specification::Unit;

/// What a token is.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    /// A bare word: a keyword, a field name or an enum token.
    Ident(String),
    /// A quoted string.
    Text(String),
    /// A number, already converted to SI by its unit suffix.
    Number {
        /// The SI value.
        value: f32,
        /// The suffix it carried, if any.
        unit: Option<Unit>,
    },
    /// `{`
    OpenBrace,
    /// `}`
    CloseBrace,
    /// `[`
    OpenBracket,
    /// `]`
    CloseBracket,
    /// `=`
    Equals,
    /// `..`
    Range,
    /// `,`
    Comma,
}

impl TokenKind {
    /// How the token is named in a diagnostic.
    pub fn describe(&self) -> String {
        match self {
            TokenKind::Ident(name) => format!("`{name}`"),
            TokenKind::Text(text) => format!("the string \"{text}\""),
            TokenKind::Number { value, .. } => format!("the number {value}"),
            TokenKind::OpenBrace => "`{`".to_string(),
            TokenKind::CloseBrace => "`}`".to_string(),
            TokenKind::OpenBracket => "`[`".to_string(),
            TokenKind::CloseBracket => "`]`".to_string(),
            TokenKind::Equals => "`=`".to_string(),
            TokenKind::Range => "`..`".to_string(),
            TokenKind::Comma => "`,`".to_string(),
        }
    }
}

/// One token and where it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    /// What it is.
    pub kind: TokenKind,
    /// Where it is.
    pub at: SourceLocation,
}

/// Turn `source` into tokens, naming the file `name` in every diagnostic.
pub fn tokenise(name: &str, source: &str) -> CourseResult<Vec<Token>> {
    let chars: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0usize;
    let mut line = 1u32;
    let mut column = 1u32;

    while i < chars.len() {
        let c = chars[i];
        let at = SourceLocation::new(name, line, column);

        // Whitespace.
        if c == '\n' {
            i += 1;
            line += 1;
            column = 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            column += 1;
            continue;
        }
        // Comments: `#` to end of line, and `//` to end of line.
        if c == '#' || (c == '/' && chars.get(i + 1) == Some(&'/')) {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
                column += 1;
            }
            continue;
        }
        // Punctuation.
        let single = match c {
            '{' => Some(TokenKind::OpenBrace),
            '}' => Some(TokenKind::CloseBrace),
            '[' => Some(TokenKind::OpenBracket),
            ']' => Some(TokenKind::CloseBracket),
            '=' => Some(TokenKind::Equals),
            ',' => Some(TokenKind::Comma),
            _ => None,
        };
        if let Some(kind) = single {
            tokens.push(Token { kind, at });
            i += 1;
            column += 1;
            continue;
        }
        if c == '.' && chars.get(i + 1) == Some(&'.') {
            tokens.push(Token {
                kind: TokenKind::Range,
                at,
            });
            i += 2;
            column += 2;
            continue;
        }
        // A quoted string.
        if c == '"' {
            let mut text = String::new();
            i += 1;
            column += 1;
            while i < chars.len() && chars[i] != '"' {
                (chars[i] == '\n')
                    .then(|| {
                        line += 1;
                        column = 0;
                    })
                    .unwrap_or(());
                text.push(chars[i]);
                i += 1;
                column += 1;
            }
            (i < chars.len()).then_some(()).ok_or_else(|| {
                CourseError::new(
                    CourseErrorCode::InvalidSyntax,
                    "a string was opened and never closed".to_string(),
                )
                .at(at.clone())
            })?;
            i += 1;
            column += 1;
            tokens.push(Token {
                kind: TokenKind::Text(text),
                at,
            });
            continue;
        }
        // A number, possibly signed, possibly with a unit suffix.
        if c.is_ascii_digit() || (c == '-' && chars.get(i + 1).is_some_and(|n| n.is_ascii_digit()))
        {
            let start = i;
            (c == '-').then(|| {
                i += 1;
                column += 1;
            });
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '_') {
                i += 1;
                column += 1;
            }
            // A decimal point, but never the `..` of a range.
            if chars.get(i) == Some(&'.') && chars.get(i + 1) != Some(&'.') {
                i += 1;
                column += 1;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                    column += 1;
                }
            }
            let digits: String = chars[start..i].iter().filter(|c| **c != '_').collect();
            let suffix_start = i;
            while i < chars.len() && chars[i].is_ascii_alphabetic() {
                i += 1;
                column += 1;
            }
            let suffix: String = chars[suffix_start..i].iter().collect();
            let raw: f32 = digits.parse().map_err(|_| {
                CourseError::new(
                    CourseErrorCode::InvalidSyntax,
                    format!("`{digits}` is not a number"),
                )
                .at(at.clone())
            })?;
            let unit = suffix
                .is_empty()
                .then_some(None)
                .map(Ok)
                .unwrap_or_else(|| {
                    Unit::parse(&suffix).map(Some).ok_or_else(|| {
                        CourseError::new(
                            CourseErrorCode::InvalidUnit,
                            format!(
                                "`{suffix}` is not a unit — this grammar knows m, km, deg, \
                                 rad, s, mps, kmh and mph"
                            ),
                        )
                        .at(at.clone())
                    })
                })?;
            tokens.push(Token {
                kind: TokenKind::Number {
                    value: unit.map(|u| u.to_si(raw)).unwrap_or(raw),
                    unit,
                },
                at,
            });
            continue;
        }
        // A bare word.
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
                column += 1;
            }
            tokens.push(Token {
                kind: TokenKind::Ident(chars[start..i].iter().collect()),
                at,
            });
            continue;
        }
        return Err(CourseError::new(
            CourseErrorCode::InvalidSyntax,
            format!("`{c}` cannot start anything this grammar understands"),
        )
        .at(at));
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(source: &str) -> Vec<TokenKind> {
        tokenise("test", source)
            .expect("tokenises")
            .into_iter()
            .map(|t| t.kind)
            .collect()
    }

    #[test]
    fn punctuation_and_words_tokenise() {
        assert_eq!(
            kinds("course { seed = 5 }"),
            vec![
                TokenKind::Ident("course".into()),
                TokenKind::OpenBrace,
                TokenKind::Ident("seed".into()),
                TokenKind::Equals,
                TokenKind::Number { value: 5.0, unit: None },
                TokenKind::CloseBrace,
            ]
        );
        assert_eq!(
            kinds("[ -1, 1 ]"),
            vec![
                TokenKind::OpenBracket,
                TokenKind::Number { value: -1.0, unit: None },
                TokenKind::Comma,
                TokenKind::Number { value: 1.0, unit: None },
                TokenKind::CloseBracket,
            ]
        );
    }

    #[test]
    fn numbers_carry_their_units_and_arrive_in_si() {
        let tokens = kinds("700m 1.5km 18deg 0.75s 180mph 90kmh 30mps 84192");
        let values: Vec<f32> = tokens
            .iter()
            .filter_map(|t| match t {
                TokenKind::Number { value, .. } => Some(*value),
                _ => None,
            })
            .collect();
        assert!((values[0] - 700.0).abs() < 1.0e-4);
        assert!((values[1] - 1_500.0).abs() < 1.0e-3);
        assert!((values[2] - 18.0f32.to_radians()).abs() < 1.0e-6);
        assert!((values[3] - 0.75).abs() < 1.0e-6);
        assert!((values[4] - 80.467_2).abs() < 1.0e-3);
        assert!((values[5] - 25.0).abs() < 1.0e-3);
        assert!((values[6] - 30.0).abs() < 1.0e-4);
        assert!((values[7] - 84_192.0).abs() < 1.0e-1);
        assert_eq!(
            tokens[7],
            TokenKind::Number {
                value: 84_192.0,
                unit: None
            },
            "a bare number carries no unit"
        );
    }

    #[test]
    fn underscores_group_digits_and_negatives_are_numbers() {
        assert_eq!(
            kinds("84_192 -3 -0.5"),
            vec![
                TokenKind::Number { value: 84_192.0, unit: None },
                TokenKind::Number { value: -3.0, unit: None },
                TokenKind::Number { value: -0.5, unit: None },
            ]
        );
    }

    #[test]
    fn a_range_is_not_a_decimal_point() {
        assert_eq!(
            kinds("90m..150m"),
            vec![
                TokenKind::Number { value: 90.0, unit: Some(Unit::Metres) },
                TokenKind::Range,
                TokenKind::Number { value: 150.0, unit: Some(Unit::Metres) },
            ]
        );
        assert_eq!(
            kinds("1.5..2.5"),
            vec![
                TokenKind::Number { value: 1.5, unit: None },
                TokenKind::Range,
                TokenKind::Number { value: 2.5, unit: None },
            ]
        );
    }

    #[test]
    fn comments_and_strings_are_handled() {
        assert_eq!(
            kinds("# a whole line\n\"burning_coast\" // trailing\nid"),
            vec![
                TokenKind::Text("burning_coast".into()),
                TokenKind::Ident("id".into()),
            ]
        );
    }

    #[test]
    fn every_token_knows_its_line_and_column() {
        let tokens = tokenise("course.brc", "course \"x\" {\n  seed = 5\n}").unwrap();
        assert_eq!(tokens[0].at.line, 1);
        assert_eq!(tokens[0].at.column, 1);
        assert_eq!(tokens[1].at.line, 1);
        assert_eq!(tokens[1].at.column, 8);
        let seed = &tokens[3];
        assert_eq!(seed.kind, TokenKind::Ident("seed".into()));
        assert_eq!(seed.at.line, 2);
        assert_eq!(seed.at.column, 3);
        assert_eq!(seed.at.source, "course.brc");
    }

    #[test]
    fn an_unknown_unit_is_named_and_rejected() {
        let err = tokenise("test", "length = 4furlongs").unwrap_err();
        assert_eq!(err.code, CourseErrorCode::InvalidUnit);
        assert!(err.message.contains("furlongs"), "{}", err.message);
        assert_eq!(err.at.map(|a| (a.line, a.column)), Some((1, 10)));
    }

    #[test]
    fn an_unterminated_string_and_a_stray_character_are_rejected() {
        let err = tokenise("test", "\"never closed").unwrap_err();
        assert_eq!(err.code, CourseErrorCode::InvalidSyntax);
        let err = tokenise("test", "course $ x").unwrap_err();
        assert_eq!(err.code, CourseErrorCode::InvalidSyntax);
        assert!(err.message.contains('$'), "{}", err.message);
        assert_eq!(err.at.map(|a| a.column), Some(8));
    }

    #[test]
    fn every_token_kind_describes_itself() {
        for kind in [
            TokenKind::Ident("x".into()),
            TokenKind::Text("y".into()),
            TokenKind::Number { value: 1.0, unit: None },
            TokenKind::OpenBrace,
            TokenKind::CloseBrace,
            TokenKind::OpenBracket,
            TokenKind::CloseBracket,
            TokenKind::Equals,
            TokenKind::Range,
            TokenKind::Comma,
        ] {
            assert!(!kind.describe().is_empty());
        }
    }
}
