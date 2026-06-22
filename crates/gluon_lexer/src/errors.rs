//! Errors that can occur during the leinxg process

use alloc::string::String;
use gluon_debug::{Located};

/// Result of one step of the lexing process, this is just a convenience
/// over having to write Result<T, `Located<LexError, FileName>`> everywhere if the token
/// type needs to change or something.
pub type LexResult<T, FileName> = Result<T, Located<LexError, FileName>>;

/// Errors that can occur while lexing.
#[derive(Debug, Clone, PartialEq)]
pub enum LexError {
    /// An unexpected character was here
    UnexpectedCharacter { character: char },

    /// An unterminated string literal
    ///
    /// hit EOF before the closing `"`.
    UnterminatedString,

    /// An unterminated macro quote
    ///
    /// hit EOF before the closing ``` `` ```.
    UnterminatedQuote,

    /// An unterminated macro quote
    ///
    /// hit EOF before the closing `)`.
    UnterminatedSplice,

    /// An unterminated string interpolation
    ///
    /// hit EOF before the closing `}`.
    UnterminatedInterp,

    /// An unterminated open `LBracket` `{`
    ///
    /// hit EOF before the closing `}`.
    UnterminatedLBrace,

    /// An unterminated open `LParen` `(`
    ///
    /// hit EOF before the closing `)`.
    UnterminatedLParen,

    /// A `}` was seen that doesn't close anything currently open
    UnmatchedRBrace,

    /// A `)` was seen that doesn't close anything currently open
    UnmatchedRParen,

    /// A numeric literal was malformed and could not be succesfully
    /// converted to an actual LitUInt/LitInt/LitFloat
    ///
    /// The reason for this happening is stored in `reason`.
    MalformedNumber { reason: String },

    /// An escape sequence inside a string was
    /// not a recognised as a valid escape sequence.
    InvalidEscape,
}
