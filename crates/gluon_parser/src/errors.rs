//! Errors that can occur during the parsing process

use alloc::string::String;
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

    /// Mixing different infix operators within the same chain without
    /// parentheses to disambiguate, e.g. `1 + 1 * 2`.
    /// 
    /// Mixing operators without parenthesis is not permitted
    /// read the `Fermion3` spec at the "Function Calls" section.
    MixedInfixOperators,

    /// A generic statement which cannot be made `public` 
    /// using the `pub` modifier
    PublicModifierOnGenericStatement,

    /// A `import` statement, there are no re-exports so
    /// this cannot be made public either!
    PublicModifierOnImport,

    /// We expect that a statement is over which is then ended
    /// with a semicolon `;` or if it's the last statement in a
    /// sequence then the `terminator` follows.
    ExpectedSeparatorOrTerminator {
        terminator: TokenKind
    },

    /// When trying to parse a pattern, it was entirely
    /// invalid/no valid starter
    InvalidPattern, 

    /// When trying to parse an `Array` `Pattern` there
    /// were multiple spreads used in that pattern!
    /// 
    /// This is dissallowed because the splitting of the
    /// spreads is like impossible to determine without
    /// including it as part of the syntax which is doing
    /// too much for patterns that should be simple.
    MoreThanOneArrayPatternSpread,

    /// A macro cannot have a return type, as it is inherently
    /// always got the same AST macroey return type, so to prevent
    /// confusion we explicitly error
    /// 
    /// See `parse_function_like_def` for where this happens
    MacroWithReturnType,
}