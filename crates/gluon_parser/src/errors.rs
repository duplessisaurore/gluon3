//! Errors that can occur during the parsing process

use gluon_debug::Located;
use gluon_lexer::TokenKind;

use crate::ast::AstNode;

/// Result of one step of the parsing process, this is just a convenience
/// over having to write Result<T, `LocatedParseError<FileName>`> everywhere
pub type ParseResult<T, FileName> = Result<T, LocatedParseError<FileName>>;

/// We want to keep source information with the ParseErrors so
/// the user of the parser can nicely output them.
pub type LocatedParseError<FileName> = Located<ParseError, FileName>;

/// Errors that can occur while parsing.
#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
    /// We expected a specific token, but found something else
    UnexpectedToken {
        expected: TokenKind,
        found: TokenKind,
    },

    /// We expected a token, but found the End Of File!
    UnexpectedEof,

    /// There is a strict operator with parenthesis rule.
    /// 
    /// Mixing operators without parenthesis is not permitted
    /// read the `Fermion3` spec at the "Function Calls" section.
    MixedOperatorsWithoutParentheses {
        expected: TokenKind,
        found: TokenKind,
    }
}