//! Errors that can occur during the leinxg process

use alloc::string::String;
use gluon_debug::Span;

use crate::Token;

/// Result of one step of the lexing process, this is just a convenience
/// over having to write Result<Token, `LexError`> everywhere if the token
/// type needs to change or something.
pub type LexResult<FileName> = Result<Token<FileName>, LexError>;

/// Errors that can occur while lexing.
#[derive(Debug, Clone, PartialEq)]
pub enum LexError {
    /// An unterminated string literal
    ///
    /// hit EOF before the closing `"`.
    UnterminatedString { start: Span },

    /// An unterminated macro quote
    ///
    /// hit EOF before the closing ``` `` ```.
    UnterminatedQuote { start: Span },

    /// An unterminated macro quote
    ///
    /// hit EOF before the closing `)`.
    UnterminatedSplice { start: Span },

    /// An unterminated string interpolation
    ///
    /// hit EOF before the closing `}`.
    UnterminatedInterp { start: Span },

    /// An unterminated open `LBracket` `{`
    ///
    /// hit EOF before the closing `}`.
    UnterminatedLBrace { start: Span },

    /// An unterminated open `LParen` `(`
    ///
    /// hit EOF before the closing `)`.
    UnterminatedLParen { start: Span },

    /// A `}` was seen that doesn't close anything currently open
    UnmatchedRBrace { at: Span },

    /// A `)` was seen that doesn't close anything currently open
    UnmatchedRParen { at: Span },

    /// A numeric literal was malformed and could not be succesfully
    /// converted to an actual LitUInt/LitInt/LitFloat
    ///
    /// The reason for this happening is stored in `reason`.
    MalformedNumber { at: Span, reason: String },

    /// An escape sequence inside a string was
    /// not a recognised as a valid escape sequence.
    InvalidEscape { at: Span },
}
