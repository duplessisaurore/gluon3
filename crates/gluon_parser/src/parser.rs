//! The actual `Parser` type
//!
//! This translates the tokenised content into a `Module`
//! with the abstract syntax tree `AstNode`'s representing
//! all program bits

use core::fmt::{Display, Debug as DebugTrait};

use alloc::{rc::Rc, string::String, vec::Vec};
use gluon_debug::{Located, SourceFile, SourceLocation, Span};
use gluon_lexer::{Token, TokenKind};

use crate::{ast::Module, errors::{LocatedParseError, ParseError, ParseResult}};

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
    pub fn advance(&mut self) -> ParseResult<Token<FileName>, FileName> {
        if self.is_at_end() || self.cursor >= self.tokens.len() {
            return Err(self.unexpected_eof());
        }

        let token = self.tokens[self.cursor].clone();
        self.cursor += 1;
        Ok(token)
    }

    /// Consume the current token as an `Ident` token kind and return the string
    /// inside the `Ident` back out, this returns the actual identifier or otherwise
    /// errors with an `UnexpectedToken` if it's not an identifier.
    pub fn expect_ident_into_inner(&mut self) -> ParseResult<String, FileName> {
        let token = self.advance()?;
        if let TokenKind::Ident(text) = token.kind {
            Ok(text)
        } else {
            Err(self.make_located(
                ParseError::UnexpectedToken {
                    expected: TokenKind::Ident(String::from("<identifier>")),
                    found: token.kind,
                },
                token.location.span,
            ))
        }
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
    ) -> ParseResult<Token<FileName>, FileName> {
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

    /// After parsing one statement in a sequence, consumes the `;` separating
    /// it from the next plus any further `;`. 
    /// 
    /// If that parsed statement is intended to be the last in some sequence of statements,
    /// which ends with a `terminatoor`, don't require a `;`. since `;` should only be mandatory
    /// between statements, not on the final one.
    fn expect_separator_or_terminator(
        &mut self,
        terminator: &TokenKind,
    ) -> ParseResult<(), FileName> {
        // Continuously consume all `;`'s.
        if self.match_token(TokenKind::Semicolon).is_some() {
            while self.match_token(TokenKind::Semicolon).is_some() {
            }
            return Ok(());
        }

        // No semicolons, check if we are either at the EoF, or if
        // the passed in terminator follows in which this is the last statement
        // of a sequence so we don't forcibly require `;`
        if self.check(terminator) || self.is_at_end() {
            return Ok(());
        }

        // Not last statement in sequence or at EoF.
        // Invalid! we expect a separator or terminator here
        Err(self.make_located(
            ParseError::ExpectedSeparatorOrTerminator {
                terminator: terminator.clone()
            },
            self.current_span(),
        ))
    }

    // = Statement Boundary Checking =

    /// This returns whether or not we are currently at a statement boundary.
    /// 
    /// A statement boundary is a location in which a statement ends, in this case
    /// we can then advance to the next statement.
    /// 
    /// This is the `Terminators` section of the `Fermion3` spec.
    pub fn at_statement_boundary(&self) -> bool {
        // EoF is always the end
        let Some(token) = self.peek_token() else {
            return true;
        };

        // A semicolon is an explicit terminator
        if token.kind == TokenKind::Semicolon {
            return true;
        }

        // Anything else is not a boundary token!
        false
    }

    /// Runs the `Parser` continuously over the source input
    /// until EOF is hit or an error occurs, returns all the
    /// tokens in a `Module`
    ///
    /// # Errors
    ///
    /// This may error in many ways!! See `ParseError`, generally
    /// if things are not in the valid sequence for the grammar as defined
    /// in the specification.
    pub fn parse_module(&mut self) -> ParseResult<Module<FileName>, FileName> {
        // Fill in this module with each top level component
        let mut module = Module {
            name: Rc::clone(&self.file),
            imports: Vec::new(),
            types: Vec::new(),
            macros: Vec::new(),
            functions: Vec::new(),
            statements: Vec::new(),
        };

        // Parse until the end of the file
        //
        // This loop is responsible for parsing all the top level module
        // statements (TLS) which may or may not be public.
        while !self.is_at_end() {
            // Check if this TLS is public or not. 
            let is_pub = self.match_token(TokenKind::KwPub).is_some();
            let next_kind = self.peek_token().map(|token| token.kind.clone());

            
            // Check for any top level statements that can be public
            // or just a general statement that should be evaluated
            match next_kind {
                Some(TokenKind::KwImport) => {
                    // Imports cannot be made "public"/rexported.
                    if is_pub {
                        return Err(self.make_located(
                            ParseError::PublicModifierOnImport,
                            self.current_span(),
                        ));
                    }

                    module.imports.push(self.parse_import()?);
                }
                Some(TokenKind::KwType) => {
                    module.types.push(self.parse_type_def(is_pub)?);
                }
                Some(TokenKind::KwFn) => {
                    module.functions.push(self.parse_function_like_def(is_pub, false)?);
                }
                Some(TokenKind::KwMacro) => {
                    self.advance()?;
                    self.expect(TokenKind::KwFn)?;
                    module.macros.push(self.parse_function_like_def(is_pub, true)?);
                }
                Some(TokenKind::KwLet) => {
                    module.statements.push(self.parse_let_binding(is_pub)?);
                }
                _ => {
                    // General statements which are just executed cannot be made public either
                    if is_pub {
                        return Err(self.make_located(
                            ParseError::PublicModifierOnGenericStatement,
                            self.current_span(),
                        ));
                    }
                    module.statements.push(self.parse_statement()?);
                }
            }

            // A separator or terminator should follow a TLS.
            self.expect_separator_or_terminator(&TokenKind::Eof)?;
        }

        Ok(module)
    }
}
