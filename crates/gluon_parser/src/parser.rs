//! The actual `Parser` type
//!
//! This translates the tokenised content into a `Module`
//! with the abstract syntax tree `AstNode`'s representing
//! all program bits

use core::fmt::{Display, Debug as DebugTrait};

use alloc::{boxed::Box, format, rc::Rc, string::String, vec::Vec};
use gluon_debug::{Located, SourceFile, SourceLocation, Span};
use gluon_lexer::{Token, TokenKind};

use crate::{ast::{AstNode, ExprKind, Literal, Module, Pattern, PatternNode}, errors::{LocatedParseError, ParseError, ParseResult}};

/// A mark which allows for storing the current position
/// and resetting to it
struct Mark(usize);

/// The function kind which can either
/// be some Function/Closure or a Macro
#[derive(PartialEq, Eq)]
/// to parse
pub enum FunctionKind {
    Function,
    Macro
}

/// The publicity of this element
#[derive(PartialEq, Eq)]
pub enum Publicity {
    Public,
    Private
}

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

    /// Consume from the current position a simple String Literal which only contains
    /// one `StrFragment` and no interpolations.
    /// 
    /// This returns the actual string fragment content or otherwise
    /// errors with an `UnexpectedToken` if it's not a simple string.
    /// 
    /// The `string_for` should describe what the string is for on the error case
    pub fn expect_simple_string_literal(&mut self, string_for: impl Into<String>) -> ParseResult<String, FileName> {
        // Must start with a StrStart
        self.expect(TokenKind::StrStart)?;

        // Followed by the string..
        let text = match self.advance()?.kind {
            TokenKind::StrFragment(text) => text,
            found => {
                return Err(self.make_located(
                    ParseError::UnexpectedToken {
                        expected: TokenKind::StrFragment(format!("<string with no interpolation for: {}>", string_for.into())),
                        found,
                    },
                    self.previous_span(),
                ))
            }
        };

        // Then only a StrEnd, no interpolations or anything!
        self.expect(TokenKind::StrEnd)?;
        Ok(text)
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
    /// 
    /// LE IMPORTANTE NOTE!:
    /// This function only consumes `;`'s IT DOES NOT CONSUME THE TERMINATOR!!!!
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
            let publicity = match self.match_token(TokenKind::KwPub) {
                Some(_) => Publicity::Public,

                // Default is private.
                None => Publicity::Private,
            };
            let next_kind = self.peek_token().map(|token| token.kind.clone());

            
            // Check for any top level statements that can be public
            // or just a general statement that should be evaluated
            match next_kind {
                Some(TokenKind::KwImport) => {
                    // Imports cannot be made "public"/rexported.
                    if publicity != Publicity::Private {
                        return Err(self.make_located(
                            ParseError::PublicModifierOnImport,
                            self.current_span(),
                        ));
                    }

                    module.imports.push(self.parse_import()?);
                }
                Some(TokenKind::KwType) => {
                    module.types.push(self.parse_type_def(publicity)?);
                }
                Some(TokenKind::KwFn) => {
                    module.functions.push(self.parse_function_like_def(publicity, FunctionKind::Function)?);
                }
                Some(TokenKind::KwMacro) => {
                    self.advance()?;
                    self.expect(TokenKind::KwFn)?;
                    module.macros.push(self.parse_function_like_def(publicity, FunctionKind::Macro)?);
                }
                Some(TokenKind::KwLet) => {
                    module.statements.push(self.parse_let_binding(publicity)?);
                }
                _ => {
                    // General statements which are just executed cannot be made public either
                    if publicity != Publicity::Private {
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

    /// Parses a sequence of `;` separated statements until `terminator`
    /// This follows the `Terminators` section where the last statement does
    /// not require an explicit `;` as it is followed by the `terminator`.
    /// 
    /// This is any possible block, including bodies of functions, 
    /// macro quotes (essentially a block if you think about it :3c)
    fn parse_block_contents(
        &mut self,
        terminator: &TokenKind,
    ) -> ParseResult<Vec<AstNode<FileName>>, FileName> {
        let mut stmts = Vec::new();

        while !self.check(terminator) && !self.is_at_end() {
            stmts.push(self.parse_statement()?);

            // Each statement should be followed by the separator `;`,
            // or be the final statement in which case is followed by the
            // terminator !!
            //
            // This function only consumes the separator so the terminator
            // would fail the loop condition on the next iteration.
            self.expect_separator_or_terminator(terminator)?;
        }
        Ok(stmts)
    }

    /// Parses a single statement at the current position,
    /// 
    /// All produced `AstNodes` are set to `Publicity::Private` 
    /// and publicity of statements is not considered.
    pub fn parse_statement(&mut self) -> ParseResult<AstNode<FileName>, FileName> {
        if self.match_token(TokenKind::KwLet).is_some() {
            self.parse_let_binding(Publicity::Private)
        } else if self.match_token(TokenKind::KwType).is_some() {
            self.parse_type_def(Publicity::Private)
        } else if self.match_token(TokenKind::KwFn).is_some() {
            self.parse_function_like_def(Publicity::Private, FunctionKind::Function)
        } else if self.match_token(TokenKind::KwMacro).is_some() {
            self.expect(TokenKind::KwFn)?;
            self.parse_function_like_def(Publicity::Private, FunctionKind::Macro)
        } else {
            self.parse_expression(Publicity::Private)
        }
    }

    /// Parses an `import <path> [as <alias>]` statement at the current position
    /// 
    /// This assumes that the `import` has not been consumed.
    fn parse_import(&mut self) -> ParseResult<AstNode<FileName>, FileName> {
        // Store the start position for the final `AstNode` total span.
        let start_span = self.expect(TokenKind::KwImport)?.location.span;

        // The path is a simple string here
        let path = self.expect_simple_string_literal("import path")?;

        // Optional alias which begins with an `as`
        let alias = if self.match_token(TokenKind::KwAs).is_some() {

            // Must be followed by an Ident
            Some(self.expect_ident_into_inner()?)
        } else {
            None
        };

        // Total span of the entire `import` statement
        let span = start_span.join(self.previous_span());
        Ok(self.make_located(ExprKind::Import { path, alias }, span))
    }

    /// Expects the next chunk of tokens to be some pattern to parse
    /// 
    /// Patterns are kind of a DSL for destructuring and `match` statements (i
    /// feel like i wrote this somehwhere else already but i forgor :skull:).
    pub fn parse_pattern(&mut self) -> ParseResult<PatternNode<FileName>, FileName> {
        // Start of the pattern for the final `PatternNode` span.
        let start_span = self.current_span();
        let token = self.advance()?;

        // Try match the specific pattern variety.
        let kind = match token.kind {
            // The pattern begins with an identifier!
            // this is the complex case (wildcard, object, enums, simple just ident?)
            TokenKind::Ident(text) => {
                if text == "_" {
                    Pattern::Wildcard

                // An `object`, since we don't have Type.Variant for enums.
                } else if self.check(&TokenKind::DelLBrace) {
                    let fields = self.parse_object_pattern_field_list()?;
                    Pattern::Object {
                        target_type: Some(Box::new(self.make_located(ExprKind::Identifier(text), start_span))),
                        fields,
                    }

                // Some kind of field access, which if followed by another identifier
                // must be an `enum` in a pattern.
                } else if self.check(&TokenKind::Dot)
                    && matches!(self.peek_token_nth(1).map(|t| &t.kind), Some(TokenKind::Ident(_)))
                {
                    self.advance()?;
                    let variant_name = self.expect_ident_into_inner()?;

                    // If there is no LBrace then this is a fieldless variant e.g Option.None
                    let fields = if self.match_token(TokenKind::DelLBrace).is_some() {
                        // Fields for enum are essentially just an object..
                        Some(self.parse_object_pattern_field_list()?)
                    } else {
                        None
                    };

                    Pattern::EnumVariant {
                        enum_type: Box::new(self.make_located(ExprKind::Identifier(text), start_span)),
                        variant_name,
                        fields,
                    }
                } else {
                    Pattern::Identifier(text)
                }
            }

            // Parse an array pattern from here `[...pattern...`
            TokenKind::DelLBracket => self.parse_array_pattern()?,

            // An unhygenic identifier, this is just a `#` (already consumed) followed
            // by the actual identifier
            TokenKind::MacroHash => Pattern::UnhygienicIdentifier(self.expect_ident_into_inner()?),

            // Simple value literals
            TokenKind::LitInt(n) => Pattern::Lit(Literal::Int(n)),
            TokenKind::LitUInt(n) => Pattern::Lit(Literal::UInt(n)),
            TokenKind::LitFloat(n) => Pattern::Lit(Literal::Float(n)),
            TokenKind::LitBool(b) => Pattern::Lit(Literal::Bool(b)),
            TokenKind::LitUnit => Pattern::Lit(Literal::Unit),

            // Simple string literals for matching (no interpolation allowed).
            TokenKind::StrStart => {
                let text = self.expect_simple_string_literal("pattern")?;
                Pattern::Lit(Literal::Str(text))
            }

            // Macro quote body for AST matching in macros to simplify macros
            // producing different output based on input.
            TokenKind::MacroQuoteStart => Pattern::Quote(Box::new(self.parse_quote_body()?)),
            
            // Nothing here starts a valid pattern
            _ => {
                return Err(self.make_located(
                    ParseError::InvalidPattern,
                    start_span,
                ))
            }
        };

        // Combine the pattern back in with the full span of the pattern
        // which is the start until the last pattern token we eated.
        let span = start_span.join(self.previous_span());
        Ok(self.make_located(kind, span))
    }
}