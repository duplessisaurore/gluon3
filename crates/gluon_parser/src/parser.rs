//! The actual `Parser` type
//!
//! This translates the tokenised content into a `Module`
//! with the abstract syntax tree `AstNode`'s representing
//! all program bits

use core::fmt::{Debug as DebugTrait, Display};

use alloc::{boxed::Box, format, rc::Rc, string::String, vec::Vec};
use gluon_debug::{Located, SourceFile, SourceLocation, Span};
use gluon_lexer::{Token, TokenKind};

use crate::{
    ast::{
        AstNode, ExprKind, Field, Literal, Module, Pattern, PatternNode, PatternObjectLikeFields,
        Publicity, TypeParams,
    },
    errors::{LocatedParseError, ParseError, ParseResult},
};

/// A mark which allows for storing the current position
/// and resetting to it
struct Mark(usize);

/// The function kind which can either
/// be some Function/Closure or a Macro
#[derive(PartialEq, Eq)]
/// to parse
pub enum FunctionKind {
    Function,
    Macro,
}

/// Restrictions on what is allowed
/// for an expression
///
/// This is important to resolve ambiguity
/// between the condition in `if`, `while`,

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
    pub fn expect_simple_string_literal(
        &mut self,
        string_for: impl Into<String>,
    ) -> ParseResult<String, FileName> {
        // Must start with a StrStart
        self.expect(TokenKind::StrStart)?;

        // Followed by the string..
        let text = match self.advance()?.kind {
            TokenKind::StrFragment(text) => text,
            found => {
                return Err(self.make_located(
                    ParseError::UnexpectedToken {
                        expected: TokenKind::StrFragment(format!(
                            "<string with no interpolation for: {}>",
                            string_for.into()
                        )),
                        found,
                    },
                    self.previous_span(),
                ));
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
    pub fn expect(&mut self, expected_kind: TokenKind) -> ParseResult<Token<FileName>, FileName> {
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
            Some(
                self.advance()
                    .expect("check already checked for a token to exist here"),
            )
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
            while self.match_token(TokenKind::Semicolon).is_some() {}
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
                terminator: terminator.clone(),
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
                    module
                        .functions
                        .push(self.parse_function_like_def(publicity, FunctionKind::Function)?);
                }
                Some(TokenKind::KwMacro) => {
                    self.advance()?;
                    self.expect(TokenKind::KwFn)?;
                    module
                        .macros
                        .push(self.parse_function_like_def(publicity, FunctionKind::Macro)?);
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
            self.parse_expression()
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
                        target_type: Some(Box::new(
                            self.make_located(ExprKind::Identifier(text), start_span),
                        )),
                        fields,
                    }

                // Some kind of field access, which if followed by another identifier
                // must be an `enum` in a pattern.
                } else if self.check(&TokenKind::Dot)
                    && matches!(
                        self.peek_token_nth(1).map(|t| &t.kind),
                        Some(TokenKind::Ident(_))
                    )
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
                        enum_type: Box::new(
                            self.make_located(ExprKind::Identifier(text), start_span),
                        ),
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
            _ => return Err(self.make_located(ParseError::InvalidPattern, start_span)),
        };

        // Combine the pattern back in with the full span of the pattern
        // which is the start until the last pattern token we eated.
        let span = start_span.join(self.previous_span());
        Ok(self.make_located(kind, span))
    }

    /// Expects the next chunk of tokens to be the object fields section of the
    /// pattern.
    ///
    /// This is in the form of some
    ///
    /// `{ field: Pattern }`
    fn parse_object_pattern_field_list(
        &mut self,
    ) -> ParseResult<PatternObjectLikeFields<FileName>, FileName> {
        // Starts with `{`
        self.expect(TokenKind::DelLBrace)?;
        let mut fields = Vec::new();

        // Keep matching fields until we hit `}`
        while !self.check(&TokenKind::DelRBrace) {
            // The name of the field must be some identifier
            let name = self.expect_ident_into_inner()?;

            // We then may optionally have a pattern that the field either destructures
            // or is aliased into
            //
            // e.g Point { x: my_x, y: my_y } or WrappedPoint { point: Point { x, y }}
            let payload = if self.match_token(TokenKind::Colon).is_some() {
                Some(self.parse_pattern()?)
            } else {
                None
            };

            fields.push(Field { name, payload });

            // Commas are required for delimiting fields in an object-like thing
            // in patterns & normally.
            if self.match_token(TokenKind::Comma).is_none() {
                break;
            }
        }

        // Ends with `}`
        // We don't build it here since empty fields {} is also
        // technically just as valid (but why?)
        self.expect(TokenKind::DelRBrace)?;
        Ok(fields)
    }

    /// Expects the next chunk of tokens to be the remaining array
    /// pattern excluding the first `[`.
    ///
    /// This is in the form of some
    ///
    /// `befores, ..rest, after]`
    fn parse_array_pattern(&mut self) -> ParseResult<Pattern<FileName>, FileName> {
        let mut before = Vec::new();
        let mut rest = None;
        let mut after = Vec::new();

        // Keep yoinking from pattern until we hit `]`
        while !self.check(&TokenKind::DelRBracket) {
            // ... spread for rest
            if self.match_token(TokenKind::DotDotDot).is_some() {
                // We can only have one spread! else its impossible to
                // determine how the user wants it spread.
                if rest.is_some() {
                    return Err(self.make_located(
                        ParseError::MoreThanOneArrayPatternSpread,
                        self.previous_span(),
                    ));
                }
                rest = Some(Box::new(self.parse_pattern()?));
            } else {
                // Non-spread, either this goes before the `rest` or after it
                // (woawww do you really need to comment that? yes.. i forget)
                // these are just simple things like `[before1, before2, ..rest, after1, after2]`
                let pat = self.parse_pattern()?;
                if rest.is_none() {
                    before.push(pat);
                } else {
                    after.push(pat);
                }
            }

            // Array fields must be delimited with a ","
            if self.match_token(TokenKind::Comma).is_none() {
                break;
            }
        }

        self.expect(TokenKind::DelRBracket)?;
        Ok(Pattern::Array {
            before,
            rest,
            after,
        })
    }

    /// Parses a quote body
    ///
    /// This is some ``
    /// <body>
    /// ``
    ///
    /// Idk how else to explain it mane/womane/othermane/person
    ///
    /// This assumes the starting two quotes "``" have already been
    /// eaten yummy yummy yummy in my tummy!!
    fn parse_quote_body(&mut self) -> ParseResult<AstNode<FileName>, FileName> {
        // The block should start from the `` span, so we yoink that.
        let start_span = self.previous_span();

        // Parse the whole statements inside the quote block
        // where the terminator of the last statement is the ``
        //
        // The quote still works on the AST just at a later phase.
        let stmts = self.parse_block_contents(&TokenKind::MacroQuoteEnd)?;

        self.expect(TokenKind::MacroQuoteEnd)?;
        let span = start_span.join(self.previous_span());
        Ok(self.make_located(ExprKind::Block(stmts), span))
    }

    /// Parses a type definition
    ///
    /// This is essentially of the form:
    /// `type <name><type params> = <node> <where (closured)> <fail (closure)> <with {methods}>`
    ///
    /// Note that where, fail and with can go anywhere but there can only be one of each "block".
    /// See the `Fermion3` specification for more examples/specifics.
    fn parse_type_def(&mut self, publicity: Publicity) -> ParseResult<AstNode<FileName>, FileName> {
        // Store the start for the full type definition span.
        let start_span = self.previous_span();

        // The <name>
        let name = self.expect_ident_into_inner()?;

        // Parse the type parameters if they exist.
        let params = if self.match_token(TokenKind::DelLAngle).is_some() {
            self.parse_type_parameters()?
        } else {
            Vec::new()
        };

        // Parse the = <node> which is any expression
        //
        // types are values so a type is produced by an expression.
        self.expect(TokenKind::Equal)?;
        let mut node = self.parse_expression()?;

        // Check in a loop for all the additions
        // `where`, `fail`, `with`.
        //
        // We essentially build a recursive `node` constructed
        // of the base with all the sections attached
        loop {
            // `Where` guard section
            if self.match_token(TokenKind::KwWhere).is_some() {
                // The closure is wrapped in a set of parens `()`
                self.expect(TokenKind::DelLParen)?;
                let guard = Box::new(self.parse_expression()?);
                self.expect(TokenKind::DelRParen)?;

                // Update the node to include this with the guard.
                let span = start_span.join(self.previous_span());
                node = self.make_located(
                    ExprKind::TypeGuard {
                        base: Box::new(node),
                        guard,
                    },
                    span,
                );

            // Guard `fail` section
            } else if self.match_token(TokenKind::KwFail).is_some() {
                // Also wrapped in `()`
                self.expect(TokenKind::DelLParen)?;
                let fail_message = Box::new(self.parse_expression()?);
                self.expect(TokenKind::DelRParen)?;

                let span = start_span.join(self.previous_span());
                node = self.make_located(
                    ExprKind::TypeFail {
                        base: Box::new(node),
                        fail_message,
                    },
                    span,
                );

            // `With` methods
            } else if self.match_token(TokenKind::KwWith).is_some() {
                // This is a bunch of function definitions wrapped in a set of
                // braces `{}`
                self.expect(TokenKind::DelLBrace)?;

                // Build the set of methods this `with` block provides
                let mut methods = Vec::new();

                // Try grab all function definitions
                while !self.check(&TokenKind::DelRBrace) && !self.is_at_end() {
                    self.expect(TokenKind::KwFn)?;
                    methods.push(
                        self.parse_function_like_def(Publicity::Private, FunctionKind::Function)?,
                    );

                    // seperator is required per method until the terminator
                    self.expect_separator_or_terminator(&TokenKind::DelRBrace)?;
                }

                // eated the terminator
                self.expect(TokenKind::DelRBrace)?;

                let span = start_span.join(self.previous_span());
                node = self.make_located(
                    ExprKind::TypeWith {
                        base: Box::new(node),
                        methods,
                    },
                    span,
                );
            } else {
                break;
            }
        }

        // Make the full type definition from the joined nodes.
        let span = start_span.join(self.previous_span());
        Ok(self.make_located(
            ExprKind::TypeDef {
                publicity,
                name,
                params,
                underlying_type: Box::new(node),
            },
            span,
        ))
    }

    /// Tries to parse type parameters in the definition
    /// of some parametric typed thing
    ///
    /// This takes the form of `<type: constraints/types, etc..>`
    /// where we expect that the left angle delim has not yet been eated.
    pub fn parse_type_parameters(&mut self) -> ParseResult<TypeParams<FileName>, FileName> {
        self.expect(TokenKind::DelLAngle)?;

        // Build the list of params, if we've specified <> then it makes sense that
        // <> on its own is an empty list.
        let mut params = Vec::new();

        // Match until the end of the type params >
        while !self.check(&TokenKind::DelRAngle) {
            // each parameter must have a name like an object
            let name = self.expect_ident_into_inner()?;

            // Optional annotation of the type
            let annotation = if self.match_token(TokenKind::Colon).is_some() {
                // The constraint can be any expression that produces a type, because it can be a parameterised type too.
                Some(Box::new(self.parse_expression()?))
            } else {
                None
            };

            params.push(Field {
                name,
                payload: annotation,
            });

            // type parameters must be `,` comma delimited.
            if self.match_token(TokenKind::Comma).is_none() {
                break;
            }
        }

        self.expect(TokenKind::DelRAngle)?;
        Ok(params)
    }

    /// Parses a function-like definition (functions or macros)
    ///
    /// This is essentially of the form:
    /// `<macro? (consumed) >fn <name?><type params>(<params>) -> <ret type> => <block>`
    fn parse_function_like_def(
        &mut self,
        publicity: Publicity,
        kind: FunctionKind,
    ) -> ParseResult<AstNode<FileName>, FileName> {
        let start_span = self.previous_span();

        // Check first for the optional name
        let name = match kind {
            // Optional name for functions since we have closures
            FunctionKind::Function => {
                if let Some(TokenKind::Ident(_)) = self.peek_token().map(|t| &t.kind) {
                    Some(self.expect_ident_into_inner()?)
                } else {
                    None
                }
            }

            // Mandatory for macros.
            FunctionKind::Macro => Some(self.expect_ident_into_inner()?),
        };

        // Type Parameters `<Type: Constraints/Types, ...>`
        let type_params = if self.match_token(TokenKind::DelLAngle).is_some() {
            self.parse_type_parameters()?
        } else {
            // Empty by default (no type params)
            Vec::new()
        };

        // Value Parameters `(Pattern: Type, ...)`
        self.expect(TokenKind::DelLParen)?;

        // Empty by default (no params)
        let mut params = Vec::new();

        // Match until the end `)`
        while !self.check(&TokenKind::DelRParen) {
            // The param name is bound, so a pattern here to permit for destructuring and things.
            let param_pat = self.parse_pattern()?;

            // Check for annotation
            let annotation = if self.match_token(TokenKind::Colon).is_some() {
                Some(Box::new(self.parse_expression()?))
            } else {
                None
            };
            params.push(crate::ast::ValueParam {
                name: param_pat,
                annotation,
            });

            // Params are delimited by `,` commas.
            if self.match_token(TokenKind::Comma).is_none() {
                break;
            }
        }
        self.expect(TokenKind::DelRParen)?;

        // Return Type `-> Type` before the block.
        let return_type = match kind {
            // Functions have a return type
            FunctionKind::Function => {
                if self.match_token(TokenKind::ThinArrow).is_some() {
                    Some(Box::new(self.parse_expression()?))
                } else {
                    None
                }
            }

            // Macros always return some AstNode thingy, so there's no real
            // reason to have return types here
            //
            // To prevent confusion we just explicitly error.
            FunctionKind::Macro => {
                if self.match_token(TokenKind::ThinArrow).is_some() {
                    return Err(
                        self.make_located(ParseError::MacroWithReturnType, self.previous_span())
                    );
                } else {
                    None
                }
            }
        };

        // Body
        self.expect(TokenKind::FatArrow)?;
        let body = Box::new(self.parse_expression()?);

        let span = start_span.join(self.previous_span());

        // Depending on the kind of function we have differing defs
        Ok(self.make_located(
            match kind {
                FunctionKind::Function => ExprKind::FunctionDef {
                    publicity,
                    name,
                    type_params,
                    params,
                    return_type,
                    body,
                },

                FunctionKind::Macro => ExprKind::MacroDef {
                    publicity,
                    // This should be handled by the above function
                    name: name
                        .expect("name should be not None here because None was handled already"),
                    type_params,
                    params,
                    body,
                },
            },
            span,
        ))
    }


    /// Parses a let binding
    ///
    /// This is essentially of the form:
    /// `let <mut?> <name/pattern> <: annotation> = <rvalue expr>`
    /// 
    /// We assume the `let` keyword has already been consumed.
    /// 
    /// my head HURTSSSS owwww
    fn parse_let_binding(&mut self, publicity: Publicity) -> ParseResult<AstNode<FileName>, FileName> {
        let start_span = self.previous_span();

        // Check if a `mut` follows the `let`
        let is_mutable = self.match_token(TokenKind::KwMut).is_some();

        // Parse the pattern to allow for destructuring
        let pattern = self.parse_pattern()?;

        // Optional annotation for guarding.
        let annotation = if self.match_token(TokenKind::Colon).is_some() {
            Some(Box::new(self.parse_expression()?))
        } else {
            None
        };

        // we must bind to some initialiser!
        // uninitialised bindings are a crime.. imo :P
        self.expect(TokenKind::Equal)?;
        let initializer = Box::new(self.parse_expression()?);

        let span = start_span.join(self.previous_span());
        Ok(self.make_located(
            ExprKind::LetBinding {
                publicity,
                is_mutable,
                pattern,
                annotation,
                initializer,
            },
            span,
        ))
    }
}
