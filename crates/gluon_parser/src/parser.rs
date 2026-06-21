//! The actual `Parser` type
//!
//! This translates the tokenised content into a `Module`
//! with the abstract syntax tree `AstNode`'s representing
//! all program bits

use core::fmt::{Display, Debug as DebugTrait};

use alloc::{rc::Rc, vec::Vec};
use gluon_debug::{Located, SourceFile, SourceLocation, Span};
use gluon_lexer::{Token, TokenKind};

use crate::errors::{LocatedParseError, ParseError};

/// A mark which allows for storing the current position
/// and resetting to it
struct Mark(usize);

/// The actual parser class itself
pub struct Parser<FileName: Display + Clone + PartialEq> {
    /// The stream of tokens produced by the Lexer
    tokens: Vec<Token<FileName>>,

    /// Our current position in the token stream
    ///
    /// We do not use Peekable or something similar since
    /// we may want to do backtracking or other things
    cursor: usize,

    /// The source file to track from which the tokens
    /// came from when building the module and SourceLocations.
    file: Rc<SourceFile<FileName>>,
}

impl<FileName: Display + Clone + PartialEq + DebugTrait> Parser<FileName> {
    /// Create a new parser over `tokens` that will parse all of the
    /// tokens into a singular `Module` for this file
    ///
    /// All of these tokens and the produced Module will be assumed to have come
    /// from the `file`passed in with the same conditions as the `Lexer`
    pub fn new(tokens: Vec<Token<FileName>>, file: Rc<SourceFile<FileName>>) -> Self {
        Self {
            tokens,
            cursor: 0,
            file,
        }
    }

    /// Look at the current token without consuming it.
    pub fn peek_token(&self) -> Option<&Token<FileName>> {
        self.tokens.get(self.cursor)
    }

    /// Look ahead at the `offset` token in the token stream without consuming it.
    pub fn peek_token_nth(&self, offset: usize) -> Option<&Token<FileName>> {
        self.tokens.get(self.cursor + offset)
    }

    /// Check if we have reached the end of the token stream.
    pub fn is_at_end(&self) -> bool {
        self.peek_token()
            .map(|token| matches!(token.kind, TokenKind::Eof))
            // If for some reason we don't have an EoF marker (lexer kaboomy?) then we have
            // also hit the EoF i suppose.
            .unwrap_or(true)
    }

    /// Consume the current token and advance the cursor.
    /// Returns an UnexpectedEof error if we try to advance past the end.
    pub fn advance(&mut self) -> Result<Token<FileName>, LocatedParseError<FileName>> {
        if self.is_at_end() || self.cursor >= self.tokens.len() {
            return Err(self.unexpected_eof());
        }

        let token = self.tokens[self.cursor].clone();
        self.cursor += 1;
        Ok(token)
    }

    /// Check if the current token matches a specific kind without consuming it.
    pub fn check(&self, kind: &TokenKind) -> bool {
        if self.is_at_end() {
            return false;
        }

        self.peek_token()
            .map(|token| &token.kind == kind)
            .unwrap_or(false)
    }

    /// Consume the current token if it matches `expected_kind`
    /// Otherwise, throw a `ParseError`
    pub fn expect(
        &mut self,
        expected_kind: TokenKind,
    ) -> Result<Token<FileName>, LocatedParseError<FileName>> {
        // Matches token.. consume and advance
        if self.check(&expected_kind) {
            self.advance()
        } else {
            // Doesnt match! return what we got with the error
            let found = self.peek_token().map(|token| token);

            // If there's no token then we've hit an unexpected EoF
            let Some(found_token) = found else {
                return Err(self.unexpected_eof());
            };

            // There's a token that we didnt expect.
            Err(self.make_located(
                ParseError::UnexpectedToken {
                    expected: expected_kind,
                    found: found_token.kind.clone(),
                },
                found_token.location.span,
            ))
        }
    }

    /// Conditionally advance if the current token matches `kind`.
    /// 
    /// Returns Some(token) if it was consumed, None otherwise.
    pub fn match_token(&mut self, kind: TokenKind) -> Option<Token<FileName>> {
        if self.check(&kind) {
            Some(self.advance().expect("check already checked for a token to exist here"))
        } else {
            None
        }
    }

    /// Gets the span of the token we just consumed.
    pub fn previous_span(&self) -> Span {
        // At the start.. no token possible
        if self.cursor == 0 {
            Span { start: 0, end: 0 }
        } else {
            self.tokens[self.cursor - 1].location.span
        }
    }

    /// Gets the span of the current token we are looking at.
    pub fn current_span(&self) -> Span {
        self.peek_token()
            .map(|t| t.location.span)
            .unwrap_or_else(|| self.previous_span())
    }

    /// Returns a new Located<T> for the kind with a source span in the current
    /// stored file of the `Lexer`.
    fn make_located<T: Clone + PartialEq>(
        &self,
        kind: T,
        source_span: Span,
    ) -> Located<T, FileName> {
        Located {
            kind,
            location: SourceLocation {
                file: Rc::clone(&self.file),
                span: source_span,
            },
        }
    }

    /// Returns an `UnexpectedEof` `ParseError` with the source location of
    /// the previous `Token`'s span
    fn unexpected_eof(&self) -> LocatedParseError<FileName> {
        self.make_located(ParseError::UnexpectedEof, self.previous_span())
    }

    /// Returns the current location of the `Parser` that can
    /// be reset to by reset(mark)
    fn mark(&self) -> Mark {
        Mark(self.cursor)
    }

    /// Resets the position of the cursor to the location
    /// specified by the Mark
    fn reset(&mut self, mark: Mark) {
        self.cursor = mark.0
    }
}
