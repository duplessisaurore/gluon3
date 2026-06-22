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
        AstNode, EnumVariantDef, ExprKind, Field, Literal, Module, ObjectElement, Pattern, PatternNode, PatternObjectLikeFields, Publicity, TypeParams
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

    /// The speculative current depth of angle brackets `<...>`
    /// 
    /// This is to prevent reinterpreting angle brackets as the
    /// binary operation instead of the actual angle brackets for
    /// parametric types, as types inherently have no notion of
    /// less/greater.
    speculative_angle_depth: usize,
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
            speculative_angle_depth: 0
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
                    self.advance()?;
                    module.types.push(self.parse_type_def(publicity)?);
                }
                Some(TokenKind::KwFn) => {
                    self.advance()?;
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
                    self.advance()?;
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

        // Parse the = <node> which is any expression or a object/enum type definition
        //
        // types are values so a type is produced by an expression.
        self.expect(TokenKind::Equal)?;
        let mut node = if self.check(&TokenKind::KwObject) {
            self.parse_object_type_def()?
        } else if self.check(&TokenKind::KwEnum) {
            self.parse_enum_type_def()?
        } else {
            self.parse_type_expression()?
        };

        // Check in a loop for all the additions
        // `where`, `fail`, `with`.
        //
        // We essentially build a recursive `node` constructed
        // of the base with all the sections attached
        loop {
            // `Where` guard section
            if self.match_token(TokenKind::KwWhere).is_some() {
                // Empty where is nothing, no point building node off of it
                if self.match_token(TokenKind::LitUnit).is_some() {
                    continue;
                }

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
                // Empty fail is nothing
                if self.match_token(TokenKind::LitUnit).is_some() {
                    continue;
                }

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

    /// Parses an `object { field: Type, ... }` type definition
    /// 
    /// Only valid as the direct RHS of a `type` definition.
    fn parse_object_type_def(&mut self) -> ParseResult<AstNode<FileName>, FileName> {
        let start_span = self.expect(TokenKind::KwObject)?.location.span;
        self.expect(TokenKind::DelLBrace)?;
        let fields = self.parse_type_object_field_list()?;
        let span = start_span.join(self.previous_span());
        Ok(self.make_located(ExprKind::ObjectTypeDef { fields }, span))
    }

    /// Parses an `enum { Variant, Variant { field: Type, ... }, ... }` type definition
    /// 
    /// Only valid as the direct RHS of a `type` definition.
    fn parse_enum_type_def(&mut self) -> ParseResult<AstNode<FileName>, FileName> {
        let start_span = self.expect(TokenKind::KwEnum)?.location.span;
        self.expect(TokenKind::DelLBrace)?;
        let variants = self.parse_enum_variant_list()?;
        let span = start_span.join(self.previous_span());
        Ok(self.make_located(ExprKind::EnumTypeDef { variants }, span))
    }

    /// Tries to parse type parameters in the definition
    /// of some parametric typed thing
    ///
    /// This takes the form of `<type: constraints/types, etc..>`
    /// where we expect that the left angle delim has been eated.
    pub fn parse_type_parameters(&mut self) -> ParseResult<TypeParams<FileName>, FileName> {
        // we are now in parametric types parsing, dont interpret `<` or `>`
        // as a possible binary operator.
        self.speculative_angle_depth += 1;

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
                Some(Box::new(self.parse_type_expression()?))
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

        // Done with parsing the parametric type
        self.speculative_angle_depth -= 1;

        self.expect(TokenKind::DelRAngle)?;
        Ok(params)
    }

    /// Attempts to parse parametric arguments
    ///
    /// This takes the form of:
    /// `<Int, String>`
    ///
    /// If it fails, it rewinds the cursor so the
    /// `<` can be parsed elsewhere
    fn try_parse_parametric_args(&mut self) -> Option<Vec<AstNode<FileName>>> {
        // Save position here
        let mark = self.mark();

        // ?
        if self.match_token(TokenKind::DelLAngle).is_none() {
            return None;
        }

        // we are now in parametric type argument parsing, dont interpret `<` or `>`
        // as a possible binary operator.
        self.speculative_angle_depth += 1;
        let result = self.try_parse_parametric_args_inner();
        self.speculative_angle_depth -= 1; 

        // If we succeeded then good, this is a parametric arguments set
        // else reset back so another function (e.g `<` or `>` as binary ops)
        // can try with the tokens
        match result {
            Some(args) => Some(args),
            None => {
                self.reset(mark);
                None
            }
        }
    }

   /// The inner component of the `try_parse_parametric_args` method,
   /// this is because there is too many returns TwT and we want to push back
   /// up the speculative angle depth 
    fn try_parse_parametric_args_inner(&mut self) -> Option<Vec<AstNode<FileName>>> {
        // Try parse all the parametric arguments, empty `<>` is just an empty vec
        let mut args = Vec::new();
        loop {
            // Parametric args can be values, so we parse up to the binary layer
            match self.parse_binary() {
                Ok(arg) => args.push(arg),
                Err(_) => return None,
            }

            // Parametric args must be delimited by `,` commas.
            if self.match_token(TokenKind::Comma).is_some() {
                continue;
            }

            // We've actually got an end here! it must be a generic.
            //
            // this is enforced by the strict operator parenthesis rule
            // as you cant have X < Y > B without X<Y> B.
            if self.match_token(TokenKind::DelRAngle).is_some() {
                return Some(args);
            }
            return None;
        }
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
        // Return Type `-> Type` before the block.
        let type_params = match kind {
            // Functions have a return type
            FunctionKind::Function => {
                if self.match_token(TokenKind::DelLAngle).is_some() {
                    self.parse_type_parameters()?
                } else {
                    // Empty by default (no type params)
                    Vec::new()
                }
            }

            // Macros do not have type parameters!!
            FunctionKind::Macro => {
                if self.match_token(TokenKind::DelLAngle).is_some() {
                    return Err(self
                        .make_located(ParseError::MacroWithTypeParameters, self.previous_span()));
                } else {
                    Vec::new()
                }
            }
        };

        // Value Parameters `(Pattern: Type, ...)`
        // Empty by default (no params)
        let mut params = Vec::new();

        // If its not some empty arguments `()` we expect the full args
        if self.match_token(TokenKind::LitUnit).is_none() {
            self.expect(TokenKind::DelLParen)?;

            // Match until the end `)`
            while !self.check(&TokenKind::DelRParen) {
                // The param name is bound, so a pattern here to permit for destructuring and things.
                let param_pat = self.parse_pattern()?;

                // Check for annotation
                let annotation: Option<Box<Located<ExprKind<FileName>, FileName>>> = if self.match_token(TokenKind::Colon).is_some() {
                    Some(Box::new(self.parse_type_expression()?))
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
        }

        // Return Type `-> Type` before the block.
        let return_type = match kind {
            // Functions have a return type
            FunctionKind::Function => {
                if self.match_token(TokenKind::ThinArrow).is_some() {
                    Some(Box::new(self.parse_type_expression()?))
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
    fn parse_let_binding(
        &mut self,
        publicity: Publicity,
    ) -> ParseResult<AstNode<FileName>, FileName> {
        let start_span = self.previous_span();

        // Check if a `mut` follows the `let`
        let is_mutable = self.match_token(TokenKind::KwMut).is_some();

        // Parse the pattern to allow for destructuring
        let pattern = self.parse_pattern()?;

        // Optional annotation for guarding.
        let annotation = if self.match_token(TokenKind::Colon).is_some() {
            Some(Box::new(self.parse_type_expression()?))
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

    /// Parses an infix function expression which is essentially an operator
    ///
    /// This is either:
    /// - a simple identifier (`+`),
    /// - a field access math.add
    /// or a parenthesised expression
    ///
    /// Returns `Ok(None)` without consuming anything if the current position
    /// doesn't start any of the above, otherwise returns the infix operator expression
    fn parse_infix_operator(&mut self) -> ParseResult<Option<AstNode<FileName>>, FileName> {
        let start_span = self.current_span();

        // Parenthesised expression beginning wth `(`
        if self.match_token(TokenKind::DelLParen).is_some() {
            let expr = self.parse_expression()?;
            self.expect(TokenKind::DelRParen)?;

            // Re-span to cover the parens themselves, not just the inner expr.
            let span = start_span.join(self.previous_span());
            return Ok(Some(self.make_located(expr.kind, span)));
        }

        // `<` / `>` are lexed as DelLAngle/DelRAngle (shared with generics),
        // but are still valid bare operators on their own and we need to consider
        // them!
        //
        // They should only be considered when we are not already trying to parse some expression
        // as parametric types, in which case the `speculative_angle_depth` > 0
        if self.speculative_angle_depth == 0 && self.match_token(TokenKind::DelLAngle).is_some() {
            return Ok(Some(self.make_located(
                ExprKind::Identifier(String::from("<")),
                start_span,
            )));
        }
        if self.speculative_angle_depth == 0 && self.match_token(TokenKind::DelRAngle).is_some() {
            return Ok(Some(self.make_located(
                ExprKind::Identifier(String::from(">")),
                start_span,
            )));
        }

        // Everything else starts with an identifier like
        // `+`, or the base of a field access like `math.add`.
        let Some(TokenKind::Ident(_)) = self.peek_token().map(|t| &t.kind) else {
            return Ok(None);
        };

        // Grab the first ident which we assume to just be a simple ident first
        let text = self.expect_ident_into_inner()?;
        let mut node = self.make_located(ExprKind::Identifier(text), start_span);

        // We keep checking each access following
        // the individual to make sure its a field access
        while self.check(&TokenKind::Dot)
            && matches!(
                self.peek_token_nth(1).map(|t| &t.kind),
                Some(TokenKind::Ident(_))
            )
        {
            self.advance()?;
            let field = self.expect_ident_into_inner()?;
            let span = start_span.join(self.previous_span());
            node = self.make_located(
                ExprKind::FieldAccess {
                    expr: Box::new(node),
                    field,
                },
                span,
            );
        }

        Ok(Some(node))
    }

    // = EXPRESSIONS =
    // The precendence heirachy is as follows
    // generally the further from the entry point is higher precedence (since we recurse first)
    //
    // assignment
    // pipeline
    // binary op
    // type op
    // unary op
    // postfix stuff
    // atomics
    //
    // parse_expression => assignment => pipeline => ... => atomics should be the order.
    // ==============

    /// The entry point for parsing any expression.
    ///
    /// This calls the lowest precedence expression kind parser.
    /// (assignment in our case :))
    pub fn parse_expression(&mut self) -> ParseResult<AstNode<FileName>, FileName> {
        self.parse_assignment()
    }

    /// Entry point for parsing a *type expression* that should result in a value
    /// of type `=`
    ///
    /// Deliberately skips `parse_assignment` as letting it fall through to 
    /// `parse_assignment` causes the annotation to swallow the rest of the statement 
    /// e.g. `let x: UInt = 0x10000000u` getting parsed as an assignment instead of 
    /// stopping after `UInt`.
    pub fn parse_type_expression(&mut self) -> ParseResult<AstNode<FileName>, FileName> {
        self.parse_pipeline()
    }

    /// Parses an assignment operation
    ///
    /// This is essentially of the form:
    /// `<target expr> = <rvalue expr>` or `<target expr> += <rvalue expr>`
    fn parse_assignment(&mut self) -> ParseResult<AstNode<FileName>, FileName> {
        // Parse LHS
        let left = self.parse_pipeline()?;

        // Check for compound assignment
        //
        // `<target> <op> = <value>`
        //
        // <op> is whatever `parse_infix_operator` can produce
        //
        // We try parse this and roll back if the rest of the
        // compound assignment doesn't fit.
        let mark = self.mark();
        if let Some(op) = self.parse_infix_operator()? {
            if self.match_token(TokenKind::Equal).is_some() {
                // Parse RHS
                let value = self.parse_assignment()?;

                // Total span will be left to the right
                let span = left.location.span.join(value.location.span);

                return Ok(self.make_located(
                    ExprKind::CompoundAssignment {
                        op: Box::new(op),
                        target: Box::new(left),
                        value: Box::new(value),
                    },
                    span,
                ));
            }

            // Wasn't actually a compound assignment, so put
            // back whatever parse_infix_operator consumed
            self.reset(mark);
        }

        // Standard assignment no in middle infix
        if self.match_token(TokenKind::Equal).is_some() {
            // grab RHS, `=` already consumed by `match_token`
            let value = self.parse_expression()?;
            let span = left.location.span.join(value.location.span);
            return Ok(self.make_located(
                ExprKind::Assignment {
                    target: Box::new(left),
                    value: Box::new(value),
                },
                span,
            ));
        }

        // No assignment here, return pipeline result.
        Ok(left)
    }

    /// Parses a pipeline chain
    ///
    /// This is essentially of the form:
    /// `<expr> |> <receiver_fn> <|> <reciever fn>*`
    fn parse_pipeline(&mut self) -> ParseResult<AstNode<FileName>, FileName> {
        // Parse the expression we are plugging into the pipeline
        let mut expr = self.parse_function_type()?;

        // Parse the pipeline chain
        loop {
            // At a statement boundary we exit (pipeline over)
            if self.at_statement_boundary() {
                break;
            };

            // Else we continue with pipelines while we can see some!
            if self.match_token(TokenKind::PipeArrow).is_some() {
                // RHS reciever
                let right = self.parse_function_type()?;

                // make pipeline with the current expr iteratively (left associative)
                let span = expr.location.span.join(right.location.span);
                expr = self.make_located(
                    ExprKind::Pipeline {
                        left: Box::new(expr),
                        right: Box::new(right),
                    },
                    span,
                );
            } else {
                break;
            }
        }

        // Return our pipeline expr if we have one,
        // or the binary result
        Ok(expr)
    }

    /// Attempts to build a string identity for an infix operator
    /// expression, so two occurrences can be compared for sameness
    /// in regards to the rule of chaining binary ops
    ///
    /// This only succeeds for the field accesses and simple idents
    /// because lowkey sameness for a normal expression is too hard
    fn infix_operator_key(node: &AstNode<FileName>) -> Option<String> {
        match &node.kind {
            ExprKind::Identifier(text) => Some(text.clone()),
            ExprKind::FieldAccess { expr, field } => {
                let expr_key = Self::infix_operator_key(expr)?;
                Some(format!("{expr_key}.{field}"))
            }
            _ => None,
        }
    }

    /// Parses a sequence of binary operations
    ///
    /// This is essentially of the form:
    /// `<expr> + <expr> + <expr>` or `<expr> * <expr>`
    ///
    /// This enforces the strict infix sequence rule.
    ///
    /// Chains of the exact same infix function are folded, but mixing
    /// different infix functions without parenthesis is an error.
    ///
    /// E.g (1 + 1 + 1 + 1) * 2 * 2 is fine, but 1 + 1 * 2 is not.
    fn parse_binary(&mut self) -> ParseResult<AstNode<FileName>, FileName> {
        let mut left = self.parse_typeops()?;

        // Tracks the identity of the operator currently being folded:
        // `None`                  => no operator taken yet (first iter)
        // `Some(None)`            => an operator was taken but we cant track it
        // `Some(Some(identity))`  => an operator was taken with identity
        //
        // See `infix_operator_key` for specifics
        let mut current_op_identity: Option<Option<String>> = None;

        loop {
            // At the boundary, dont keep taking
            if self.at_statement_boundary() {
                break;
            }

            // Try match the same infix operator (or the first)
            let before_op = self.mark();
            let Some(op) = self.parse_infix_operator()? else {
                break;
            };

            // Operators followed by an `=` are compound assignment so rewind and escape
            // and let assignment handle it if we have an `=`
            // 
            // != and stuff are specially lexed things, i mean it could be compound not
            // equals assignment, but that doesnt make sense considering history
            if self.match_token(TokenKind::Equal).is_some() {
                self.reset(before_op);
                break;
            }

            let op_identity = Self::infix_operator_key(&op);

            // If we already had some prior op in the chain...
            if let Some(prev_key) = &current_op_identity {
                // Only fold when we know exactly that this is the same op
                // by identity to enforce the rule.
                //
                // If either side is opaque, we treat them as different
                // bcz lowkey theres no way to know.
                let same = matches!((prev_key, &op_identity), (Some(a), Some(b)) if a == b);
                if !same {
                    // Different operator than the prior one!
                    // this is illegal without parenthesis according to our rule.
                    return Err(
                        self.make_located(ParseError::MixedInfixOperators {
                            prev_ident: prev_key.clone(),
                            cur_ident: op_identity

                        }, op.location.span)
                    );
                }
            }

            // parse RHS and fold
            let right = self.parse_typeops()?;
            let span = left.location.span.join(right.location.span);
            left = self.make_located(
                ExprKind::BinaryOp {
                    op: Box::new(op),
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            );

            // Update identity to track the identity of the operator.
            current_op_identity = Some(op_identity);
        }

        // Return our binary op expr if we have one,
        // or the typeops result
        Ok(left)
    }

    /// Parses type operations
    ///
    /// This is essentially of the form:
    /// `<expr> as <Type>` or `<expr> is <Type>`
    fn parse_typeops(&mut self) -> ParseResult<AstNode<FileName>, FileName> {
        // LHS
        let mut expr = self.parse_unary()?;

        loop {
            // At boundary, dont consume any more
            if self.at_statement_boundary() {
                break;
            }

            // `as <Type>`
            if self.match_token(TokenKind::KwAs).is_some() {
                let target_type = self.parse_unary()?;
                let span = expr.location.span.join(target_type.location.span);

                // fold RHS type
                expr = self.make_located(
                    ExprKind::TypeCast {
                        expr: Box::new(expr),
                        target_type: Box::new(target_type),
                    },
                    span,
                );

            // `is <Type>`
            } else if self.match_token(TokenKind::KwIs).is_some() {
                let target_type = self.parse_unary()?;
                let span = expr.location.span.join(target_type.location.span);

                // fold RHS type
                expr = self.make_located(
                    ExprKind::TypeCheck {
                        expr: Box::new(expr),
                        target_type: Box::new(target_type),
                    },
                    span,
                );

            // no more chain
            } else {
                break;
            }
        }

        // Return our typeops expr if we have one,
        // or the unary LHS result
        Ok(expr)
    }

    /// Sadly for unary operators we need a restricted
    /// reserved set, as else who's to say
    /// x + 5 doesn't mean x (+5) as the unary +?
    const UNARY_OPERATORS: &[&str] = &["-", "!"];

    /// Parses a unary prefix operation
    ///
    /// This is essentially of the form:
    /// `<UNARY_OPERATORS><expr>`
    fn parse_unary(&mut self) -> ParseResult<AstNode<FileName>, FileName> {
        // Check if its a unary operator..
        let looks_like_prefix_op = matches!(
            self.peek_token().map(|t| &t.kind),
            Some(TokenKind::Ident(text)) if Self::UNARY_OPERATORS.contains(&text.as_str())
        );

        // We have one!
        if looks_like_prefix_op {
            let start_span = self.current_span();

            // Parse the infix unary operator
            let op = self
                .parse_infix_operator()?
                .expect("already checked the current token is a recognised unary operator");

            // Parse the thing we're applying it to, safe because we've already
            // eaten the unary op
            let expr = Box::new(self.parse_unary()?);
            let span = start_span.join(expr.location.span);

            return Ok(self.make_located(
                ExprKind::UnaryOp {
                    op: Box::new(op),
                    expr,
                },
                span,
            ));
        }

        // Not a unary operator, just fall through and let postfix handle it
        self.parse_postfix()
    }

    /// Parses postfix modifiers and structural chaining
    ///
    /// This parses:
    /// - Field Accesses `.field`
    /// - Function Calls `()`
    /// - Array Indexing `[]`
    /// - Parametric Types `<>`
    ///
    fn parse_postfix(&mut self) -> ParseResult<AstNode<FileName>, FileName> {
        // Parse LHS (this postfix applies to the LHS)
        let mut expr = self.parse_atom()?;

        loop {
            // Postfix ended since we are at the boundary
            if self.at_statement_boundary() {
                break;
            }

            // Keep matching as many postfix as we can.
            match self.peek_token().map(|t| &t.kind) {
                // Try see if it's a generic (LAngle, RAngle)
                Some(TokenKind::DelLAngle) => {
                    if let Some(args) = self.try_parse_parametric_args() {
                        let span = expr.location.span.join(self.previous_span());
                        expr = self.make_located(
                            ExprKind::Parametric {
                                target: Box::new(expr),
                                arguments: args,
                            },
                            span,
                        );
                        continue;
                    } else {
                        // It's a `<` as in less than, let the BinaryOp handle this
                        // (fall back down precedence)
                        break;
                    }
                }

                // Function Calls `(args..)` or `()`
                Some(TokenKind::DelLParen) | Some(TokenKind::LitUnit) => {
                    // Parse the function arguments
                    let arguments = self.parse_fn_arguments()?;

                    let span = expr.location.span.join(self.previous_span());
                    expr = self.make_located(
                        ExprKind::Call {
                            callee: Box::new(expr),
                            arguments,
                        },
                        span,
                    );
                }

                // Indexing and Slicing
                Some(TokenKind::DelLBracket) => {
                    self.advance()?;

                    // Check whether or not we have some current element
                    // is the start of a slice (.. here means No slice start)
                    let start = if self.check(&TokenKind::DotDot) {
                        None
                    } else {
                        Some(Box::new(self.parse_pipeline()?))
                    };

                    // Now we enter the slice, `parse_pipeline` above would parse
                    // the index/start if there was one.
                    if self.match_token(TokenKind::DotDot).is_some() {
                        // Grab the optional end after the ..
                        let end = if self.check(&TokenKind::DelRBracket) {
                            None
                        } else {
                            Some(Box::new(self.parse_pipeline()?))
                        };
                        self.expect(TokenKind::DelRBracket)?;

                        // Put start and end together for the slice
                        let span = expr.location.span.join(self.previous_span());
                        expr = self.make_located(
                            ExprKind::Slice {
                                array: Box::new(expr),
                                start,
                                end,
                            },
                            span,
                        );
                    } else {
                        // In this case the start was just a index because we dont have
                        // the slice ..
                        self.expect(TokenKind::DelRBracket)?;
                        let span = expr.location.span.join(self.previous_span());
                        expr = self.make_located(
                            ExprKind::IndexAccess {
                                expr: Box::new(expr),
                                index: start.unwrap(),
                            },
                            span,
                        );
                    }
                }

                // Field Access / Enum Variants
                Some(TokenKind::Dot) => {
                    self.advance()?;

                    // The field access/enum variant `.` must be followed by an ident
                    let field = self.expect_ident_into_inner()?;

                    // Check for an Enum Variant Literal `Shape.Circle { radius }`
                    if self.check(&TokenKind::DelLBrace) {
                        // Since we have a `{}` it must be, so full steam ahead!
                        self.advance()?;

                        // Grab all of the elements in this literal
                        let elements = self.parse_object_literal_elements()?;

                        let span = expr.location.span.join(self.previous_span());
                        expr = self.make_located(
                            ExprKind::EnumVariantLiteral {
                                enum_type: Box::new(expr),
                                variant_name: field,
                                elements: Some(elements),
                            },
                            span,
                        );
                    } else {
                        // We just have a simple field access, no enum variant because no `{}`
                        let span = expr.location.span.join(self.previous_span());
                        expr = self.make_located(
                            ExprKind::FieldAccess {
                                expr: Box::new(expr),
                                field,
                            },
                            span,
                        );
                    }
                }

                _ => break,
            }
        }

        // Return the postfix expression
        // or just the base `primary` result if there
        // was no postfix..
        Ok(expr)
    }

    /// Parses the inner object elements of a literal, this assumes
    /// that the `{` starting the object has been consumed.
    ///
    /// This takes the form of:
    /// `{ field: value }`
    fn parse_object_literal_elements(
        &mut self,
    ) -> ParseResult<Vec<ObjectElement<FileName>>, FileName> {
        // Empty objects `{}` are valid
        let mut elements = Vec::new();

        // Parse until the final `}``
        while !self.check(&TokenKind::DelRBrace) {
            // Account for spreads.
            if self.match_token(TokenKind::DotDotDot).is_some() {
                elements.push(ObjectElement::Spread(self.parse_expression()?));
            } else {
                // Normal field: value case, field must be an ident
                let name = self.expect_ident_into_inner()?;
                self.expect(TokenKind::Colon)?;

                let value = self.parse_expression()?;
                elements.push(ObjectElement::Field(Field {
                    name,
                    payload: Box::new(value),
                }));
            }

            // Fields must be `,` delimited!
            if self.match_token(TokenKind::Comma).is_none() {
                break;
            }
        }

        // Object literals must be ended by a `}`
        self.expect(TokenKind::DelRBrace)?;
        Ok(elements)
    }

    /// Parses an atomic expression
    ///
    /// This is the bottom of evaluation. It parses base values:
    ///
    /// Literals, Identifiers, Parenthesized sub-expressions, complex structures
    /// `if`, `match`, loops, and macros `@macro`, `$splice`.
    fn parse_atom(&mut self) -> ParseResult<AstNode<FileName>, FileName> {
        let start_span = self.current_span();

        // Grab the atomic token.
        let token = self.advance()?;

        let kind = match token.kind {
            // Literals
            TokenKind::LitInt(n) => ExprKind::Lit(Literal::Int(n)),
            TokenKind::LitUInt(n) => ExprKind::Lit(Literal::UInt(n)),
            TokenKind::LitFloat(n) => ExprKind::Lit(Literal::Float(n)),
            TokenKind::LitBool(b) => ExprKind::Lit(Literal::Bool(b)),
            TokenKind::LitUnit => {
                if self.match_token(TokenKind::ThinArrow).is_some() {
                    // `() -> <return>` is a zero parameter function type rather than being a Unit lit
                    let return_type = Box::new(self.parse_type_expression()?);
                    ExprKind::FunctionType {
                        params: Vec::new(),
                        return_type,
                    }
                } else {
                    ExprKind::Lit(Literal::Unit)
                }
            }

            // Strings with interpolation handled
            TokenKind::StrStart => {
                // Generally a string with interpolation is the string fragments and
                // the expressions of the interpolations.
                let mut parts = Vec::new();

                // Track if we have an interp at all to shove only a simple StrLiteral
                // vs the full StrInterp with vec.
                let mut has_interp = false;

                loop {
                    // Grab the current part's token
                    let part_token = self.advance()?;
                    match part_token.kind {
                        // A simple fragment, just add it to the parts
                        TokenKind::StrFragment(text) => {
                            let span = part_token.location.span;
                            parts.push(self.make_located(ExprKind::Lit(Literal::Str(text)), span));
                        }
                        // The start of some interpolation region
                        TokenKind::StrInterpStart => {
                            has_interp = true;
                            // Followed by the expression and then the end of the region
                            let expr = self.parse_expression()?;
                            self.expect(TokenKind::StrInterpEnd)?;
                            parts.push(expr);
                        }
                        // End of the string
                        TokenKind::StrEnd => break,
                        // ????
                        _ => {
                            return Err(self.make_located(
                                ParseError::UnexpectedStringToken,
                                part_token.location.span,
                            ));
                        }
                    }
                }

                // If we have interpolation then don't simplify
                if has_interp {
                    ExprKind::StrInterp(parts)
                } else {
                    // Collapse to Str Literal to simplify it if there isnt an interp
                    match parts.into_iter().next() {
                        Some(node) => node.kind,
                        None => ExprKind::Lit(Literal::Str(alloc::string::String::new())),
                    }
                }
            }
            // Identifiers (and potential Object Literals)
            // All enum variants are handled by the `FieldAccess` possiblity
            // in postfix
            TokenKind::Ident(text) => {
                // We have an object literal!
                if self.match_token(TokenKind::DelLBrace).is_some() {
                    // Parse all its elements..
                    let elements = self.parse_object_literal_elements()?;

                    let target = self.make_located(ExprKind::Identifier(text), start_span);
                    ExprKind::ObjectLiteral {
                        target_type: Some(Box::new(target)),
                        elements,
                    }
                // Simple placeholder here (for pipelines or something)
                } else if text == "_" {
                    ExprKind::Placeholder
                } else {
                    ExprKind::Identifier(text)
                }
            }

            // Control Flow
            TokenKind::KwIf => {
                // if <condition> then <expr> else <expr>
                let condition = Box::new(self.parse_expression()?);
                self.expect(TokenKind::KwThen)?;

                // then and optional else.
                let then_branch = Box::new(self.parse_expression()?);
                let else_branch = if self.match_token(TokenKind::KwElse).is_some() {
                    Some(Box::new(self.parse_expression()?))
                } else {
                    None
                };

                ExprKind::If {
                    condition,
                    then_branch,
                    else_branch,
                }
            }

            TokenKind::KwMatch => {
                // match <subject> with { arms ... }
                let subject = Box::new(self.parse_expression()?);
                self.expect(TokenKind::KwWith)?;
                self.expect(TokenKind::DelLBrace)?;

                // Grab all of the arms.
                let mut arms = Vec::new();

                // Arms end at the next `}`
                while !self.check(&TokenKind::DelRBrace) {
                    // Pattern for this arm (required)
                    let pattern = self.parse_pattern()?;

                    // Optional if guard (if <condition>)
                    let guard = if self.match_token(TokenKind::KwIf).is_some() {
                        Some(Box::new(self.parse_expression()?))
                    } else {
                        None
                    };

                    // => body
                    self.expect(TokenKind::FatArrow)?;
                    let body = Box::new(self.parse_expression()?);
                    arms.push(crate::ast::MatchArm {
                        pattern,
                        guard,
                        body,
                    });

                    // Require arms to be `,` delimited
                    if self.match_token(TokenKind::Comma).is_none() {
                        break;
                    }
                }
                self.expect(TokenKind::DelRBrace)?;
                ExprKind::Match { subject, arms }
            }

            TokenKind::KwWhile => {
                // while <condition> do <body>
                let condition = Box::new(self.parse_expression()?);
                self.expect(TokenKind::KwDo)?;
                let body = Box::new(self.parse_expression()?);
                ExprKind::While { condition, body }
            }

            TokenKind::KwFor => {
                // for <pattern> in <iterable> do <body>
                let pattern = self.parse_pattern()?;
                self.expect(TokenKind::KwIn)?;
                let iterable = Box::new(self.parse_expression()?);
                self.expect(TokenKind::KwDo)?;
                let body = Box::new(self.parse_expression()?);
                ExprKind::For {
                    pattern,
                    iterable,
                    body,
                }
            }

            // Blocks `{ ... }`
            TokenKind::DelLBrace => {
                let stmts = self.parse_block_contents(&TokenKind::DelRBrace)?;
                self.expect(TokenKind::DelRBrace)?;
                ExprKind::Block(stmts)
            }

            // Parentheses `( ... )`
            //
            // This can be a grouped expression or the param list
            // of a function type
            TokenKind::DelLParen => {
                // Parse all elements in the parens `(` `)`
                let mut items = Vec::new();
                if !self.check(&TokenKind::DelRParen) {
                    loop {
                        items.push(self.parse_expression()?);

                        // Function param list types must be `,` comma delimited.
                        if self.match_token(TokenKind::Comma).is_none() {
                            break;
                        }
                    }
                }
                self.expect(TokenKind::DelRParen)?;

                // Check if it is a function type
                if self.match_token(TokenKind::ThinArrow).is_some() {
                    // `(<params>) -> <return>`
                    let return_type = Box::new(self.parse_type_expression()?);
                    ExprKind::FunctionType {
                        params: items,
                        return_type,
                    }
                } else {
                    // Not a function type, there are no tuples
                    // so this is invalid if > 1 item.
                    match items.len() {
                        1 => return Ok(items.into_iter().next().expect("just checked len is 1")),
                        _ => {
                            return Err(self.make_located(
                                ParseError::UnexpectedTuple,
                                self.current_span(),
                            ));
                        }
                    }
                }
            }

            // Arrays `[1, 2, ...xs]`
            TokenKind::DelLBracket => {
                // All of the elements of this array literal
                let mut elements = Vec::new();
                while !self.check(&TokenKind::DelRBracket) {
                    if self.match_token(TokenKind::DotDotDot).is_some() {
                        // Spread of some expression
                        elements.push(crate::ast::ArrayElement::Spread(self.parse_expression()?));
                    } else {
                        // Normal value
                        elements.push(crate::ast::ArrayElement::Normal(self.parse_expression()?));
                    }

                    // Array literal elements must be comma delimited.
                    if self.match_token(TokenKind::Comma).is_none() {
                        break;
                    }
                }
                self.expect(TokenKind::DelRBracket)?;
                ExprKind::ArrayLiteral(elements)
            }

            // Jumps

            // `break` & `return` have an optional value they return
            TokenKind::KwBreak => ExprKind::Break(if self.at_statement_boundary() {
                None
            } else {
                Some(Box::new(self.parse_expression()?))
            }),
            TokenKind::KwReturn => ExprKind::Return(if self.at_statement_boundary() {
                None
            } else {
                Some(Box::new(self.parse_expression()?))
            }),

            // A value must follow `raise` and `defer`
            TokenKind::KwRaise => ExprKind::Raise(Box::new(self.parse_expression()?)),
            TokenKind::KwDefer => ExprKind::Defer(Box::new(self.parse_expression()?)),
            TokenKind::KwContinue => ExprKind::Continue,

            // `Try`/`Catch`
            TokenKind::KwTry => {
                // try <body> catch <pattern> do <handler>
                let try_body = Box::new(self.parse_expression()?);
                self.expect(TokenKind::KwCatch)?;
                let error_binding = self.parse_pattern()?;
                self.expect(TokenKind::KwThen)?;
                let catch_body = Box::new(self.parse_expression()?);

                ExprKind::TryCatch {
                    try_body,
                    error_binding,
                    catch_body,
                }
            }

            // Macros
            TokenKind::MacroHash => ExprKind::UnhygienicIdentifier(self.expect_ident_into_inner()?),

            // Slice an expression here
            TokenKind::MacroSpliceStart => {
                let expr = self.parse_expression()?;
                self.expect(TokenKind::MacroSpliceEnd)?;
                ExprKind::MacroSplice(Box::new(expr))
            }

            // Similar to function call but without type things.
            TokenKind::MacroAt => {
                let callee = Box::new(self.parse_postfix()?);
                // Grab all arguments
                let arguments = self.parse_fn_arguments()?;
                ExprKind::MacroInvoke {
                    macro_target: callee,
                    arguments,
                }
            }

            found => {
                return Err(self.make_located(
                    ParseError::UnexpectedNonExpressionToken { found },
                    start_span,
                ));
            }
        };

        let span = start_span.join(self.previous_span());
        Ok(self.make_located(kind, span))
    }

    /// Parse the value arguments to a function
    ///
    /// This is of the form of some
    /// `(arg, arg, arg)`
    ///
    /// The `(` is expected to NOT have been consumed by here.
    fn parse_fn_arguments(&mut self) -> ParseResult<Vec<AstNode<FileName>>, FileName> {
        // Is this an empty set of arguments `()`
        if self.match_token(TokenKind::LitUnit).is_some() {
            return Ok(Vec::new());
        }
        
        // Consume the `(`
        self.expect(TokenKind::DelLParen)?;

        // All the arguments are expressions
        let mut arguments = Vec::new();

        // Keep grabbing arguments until we hit the `)`
        while !self.check(&TokenKind::DelRParen) {
            arguments.push(self.parse_expression()?);

            // Arguments must be `,` delimited.
            if self.match_token(TokenKind::Comma).is_none() {
                break;
            }
        }
        self.expect(TokenKind::DelRParen)?;

        Ok(arguments)
    }

    /// Parses a function type
    ///
    /// This is essentially of the form:
    /// `<param-type> -> <return-type>`
    ///
    /// The parenthesised mutli parameter form is `(Int, Int) -> Int`
    ///
    /// `() -> Int` is handled inside `parse_atom`, since only a
    /// parenthesised group can hold more than one param
    ///
    /// This level only handles the bare single-param form `Int -> Int`
    ///
    /// This is right associative.
    fn parse_function_type(&mut self) -> ParseResult<AstNode<FileName>, FileName> {
        let start_span = self.current_span();
        // LHS
        let left = self.parse_binary()?;

        // Check for function type arrow
        if self.match_token(TokenKind::ThinArrow).is_some() {
            // Parse the RHS
            let return_type = Box::new(self.parse_function_type()?);
            let span = start_span.join(self.previous_span());

            // The LHS is the params that produce this return type
            let mut params = Vec::new();
            params.push(left);

            return Ok(self.make_located(
                ExprKind::FunctionType {
                    params,
                    return_type,
                },
                span,
            ));
        }

        Ok(left)
    }

    /// Parses an object type `object { name: Type, ... }` for object type defs.
    /// 
    /// Assumes the `{` has already been consumed.
    fn parse_type_object_field_list(&mut self) -> ParseResult<Vec<Field<Box<AstNode<FileName>>>>, FileName> {
        // Grab each field until the end
        let mut fields = Vec::new();
        while !self.check(&TokenKind::DelRBrace) {

            // Name of the field
            let name = self.expect_ident_into_inner()?;

            // The object field must have some type
            self.expect(TokenKind::Colon)?;
            let field_type = self.parse_type_expression()?;

            // Add new field
            fields.push(Field { name, payload: Box::new(field_type) });

            // We require fields to be `,` comma delimited.
            if self.match_token(TokenKind::Comma).is_none() {
                break;
            }
        }
        self.expect(TokenKind::DelRBrace)?;
        Ok(fields)
    }

    /// Parses `enum { Variant, Variant { field: Type, ... }, ... }` for enum type defs.
    /// 
    /// Assumes the `{` has already been consumed.
    fn parse_enum_variant_list(&mut self) -> ParseResult<Vec<EnumVariantDef<FileName>>, FileName> {
        // Get all variants for this enum's def
        let mut variants = Vec::new();

        // Variants are terminated by a }
        while !self.check(&TokenKind::DelRBrace) {
            // Each variant must have some name
            let name = self.expect_ident_into_inner()?;

            // Does the variant have fields? if so get em too nyaa~ :3
            let fields = if self.match_token(TokenKind::DelLBrace).is_some() {
                Some(self.parse_type_object_field_list()?)
            } else {
                None
            };

            // Add to variants
            variants.push(EnumVariantDef { name, fields });

            // We require variants to be `,` delimited.
            if self.match_token(TokenKind::Comma).is_none() {
                break;
            }
        }

        self.expect(TokenKind::DelRBrace)?;
        Ok(variants)
    }
}
