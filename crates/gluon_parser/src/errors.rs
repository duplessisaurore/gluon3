//! Errors that can occur during the parsing process

use crate::ast::AstNode;

/// Result of one step of the parsing process, this is just a convenience
/// over having to write Result<AstNode<FileName>, `LexError`> everywhere if the token
/// type needs to change or something.
pub type LexResult<FileName> = Result<AstNode<FileName>, ParseError>;

/// Errors that can occur while parsing.
#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
}
