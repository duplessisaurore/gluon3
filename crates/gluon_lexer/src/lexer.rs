//! The actual `Lexer` type
//!
//! This translates the source file content into the tokens
//! specified by `TokenKind` for one source

use core::{fmt::Display, str::Chars};

use alloc::{rc::Rc, string::String, vec::Vec};
use gluon_debug::{Located, SourceFile, SourceLocation, Span};

use crate::{
    errors::{LexError, LexResult},
    tokens::{Token, TokenKind},
};

/// Modes that the `Lexer` can operate in a stack
/// to handle nested contexts for differing parts of the
/// kinds of tokens
#[derive(Clone, Copy)]
enum LexerMode {
    /// Ordinary code tokenisation, we arent in any context
    /// that requires special lexing
    Normal,

    /// Inside the "text literal" region of a string literal
    /// this is between the opening `"` and any `${...}` boundaries
    /// and the closing `"` which should just be tokenised as `StrFragment`
    StrTextLiteral { start: usize },

    /// Inside the interpolation region of a string literal
    /// which is marked by the `${...}` symbols.
    StrInterp { start: usize },

    /// Inside a macro's quote region which is marked by the
    /// quote symbols:
    ///
    /// ```
    /// ``
    /// ```
    Quote { start: usize },

    /// Inside a `$( ... )` macro splice region which is similar to
    /// string interpolation but for macro quotes.
    Splice { start: usize },

    /// Inside a block/`{...}` bracket region, this is structural
    /// tracking for keeping nested depth
    Brace { start: usize },

    /// Inside a parenthesis/`(...)` region, this is structural
    /// tracking for keeping nested depth
    Paren { start: usize },
}

/// The actual lexer class itself
///
/// The mode that the lexer is in is stored in
/// a stack as there may be many different places where
/// we have nested either tokens or literal strings.
///
/// This dictates the current execution "mode", see
/// `LexerMode` for explanation on each mode.
pub struct Lexer<'src, FileName: Display + Clone> {
    /// The source file content we are currently lexing
    ///
    /// We need to keep this around for calculating the
    /// offset into the file while leveraging Chars
    ///
    /// We cant use `CharIndicies` since slicing with that
    /// doesnt really work nicely with the actual offsets
    /// into the source
    source: &'src str,

    /// Our cursor over the source file
    chars: Chars<'src>,

    /// The source file to track from which the contents
    /// to lex came from
    file: Rc<SourceFile<FileName>>,

    /// Mode stack for tracking the current mode the lexer is
    /// in, will always have at least one mode (`Mode::Normal`) at
    /// the bottom.
    modes: Vec<LexerMode>,
}

impl<'src, FileName: Display + Clone + PartialEq> Lexer<'src, FileName> {
    /// Create a new lexer over `source` that will lex all of the
    /// textual contents into `Tokens`
    ///
    /// All of these tokens will be assumed to have come from the `file`
    /// passed in, and for attached debug information will be stated to have
    /// come from the `file`.
    #[must_use]
    pub fn new(source: &'src str, file: Rc<SourceFile<FileName>>) -> Self {
        Self {
            source,
            file,
            chars: source.chars(),
            // The bottom mode as specified is Normal
            modes: alloc::vec![LexerMode::Normal],
        }
    }

    /// Returns a new Located<T> for the kind with a source span in the current
    /// stored file of the `Lexer`.
    fn make_located<T: Clone + PartialEq>(&self, kind: T, source_span: Span) -> Located<T, FileName> {
        Located {
            kind,
            location: SourceLocation {
                file: Rc::clone(&self.file),
                span: source_span,
            },
        }
    }

    /// Returns the current byte offset in the source
    fn current_pos(&self) -> usize {
        self.source.len() - self.chars.as_str().len()
    }

    /// Span from `start` to the current position.
    fn span_from(&self, start: usize) -> Span {
        Span {
            start,
            end: self.current_pos(),
        }
    }

    /// Clone the Chars iterator over the string
    /// (this is essentially free!)
    fn clone_chars(&self) -> Chars<'src> {
        // This is a slice::Iter
        // cloning this does NOT clone each element, instead
        // since a slice is more like a pointer into the source
        // we are kind of just cloning that pointer, so its basically
        // free
        self.chars.clone()
    }

    /// Peek the current char without consuming it.
    fn peek_char(&self) -> Option<char> {
        // By cloning the iterator, we advance the cloned one
        // instead of the actual one
        self.clone_chars().next()
    }

    /// Peek the `nth` char after the current one without consuming anything.
    fn peek_char_nth(&self, nth: usize) -> Option<char> {
        self.clone_chars().nth(nth)
    }

    /// Consume and return the current char
    ///
    /// This advances the cursor
    fn advance(&mut self) -> Option<char> {
        self.chars.next()
    }

    /// If the unconsumed text starts with `s`,
    /// consume it and return true.
    ///
    /// Else returns false and does not consume.
    ///
    /// Essentially tries to advance the cursor
    /// by some &str and returns the result as true/false
    fn try_advance_str(&mut self, s: &str) -> bool {
        let from_current = self.chars.as_str();
        if let Some(remaining) = from_current.strip_prefix(s) {
            // Fast forward the iterator by slicing the remaining string
            // and re-charsing it.
            //
            // This is why we cant use CharsIndices because the indices would
            // come from the new slice instead of the source, but chars lets
            // us do it yippie!!!
            self.chars = remaining.chars();
            true
        } else {
            false
        }
    }

    /// Returns the latest element in the mode stack
    /// of the lexer
    fn current_mode(&self) -> LexerMode {
        // There should always be at least one mode else something
        // bad has happened! (popped more than pushed)
        *self.modes.last().expect("mode stack not empty")
    }

    /// Adds a new mode to the mode stack of the `Lexer`
    fn push_mode(&mut self, mode: LexerMode) {
        self.modes.push(mode);
    }

    /// Pop the current mode.
    ///
    /// Panics if it would empty the stack, since `Normal` at the b
    /// ottom must never be popped and if one does, then its popping more
    /// than pushing (bad!!)
    fn pop_mode(&mut self) -> LexerMode {
        debug_assert!(
            self.modes.len() > 1,
            "trying to pop the base Normal lexer mode!"
        );
        self.modes.pop().expect("mode stack not empty")
    }

    /// These are reserved symbols which can never end up in an identifer
    /// even if its a keyword/symbolic/normal.
    ///
    /// This is because if they could.. it'd probably make the codebase
    /// more confusing and harder to parse from a scan of the eyes.
    fn is_reserved(c: char) -> bool {
        matches!(
            c,
            '(' | ')'
                | '{'
                | '}'
                | '['
                | ']'
                | ','
                | ';'
                | ':'
                | '"'
                | '`'
                | '@'
                | '#'
                | '.'
                | '='
                | '<'
                | '>'
                | '$'
                | '+'
                | '-'
                | '*'
                | '/' 
                | '!' 
                | '&' 
                | '|' 
                | '%' 
                | '^' 
                | '~'
        )
    }

    /// Runs the `Lexer` once to attempt to produce
    /// the next `Token` depending on the current mode.
    ///
    /// # Errors
    ///
    /// This may error in many ways!! See `LexError`, generally
    /// if things are unterminated or if there's an invalid literal
    /// or with some garbage on the end of the number
    pub fn next_token(&mut self) -> LexResult<FileName> {
        match self.current_mode() {
            LexerMode::StrTextLiteral { start: _ } => self.lex_str_fragment_or_boundary(),
            _ => self.lex_normal_token(),
        }
    }

    /// Runs the `Lexer` continuously over the source input
    /// until EOF is hit or an error occurs, returns all the
    /// tokens in a Vec
    ///
    /// # Errors
    ///
    /// This may error in many ways!! See `LexError`, generally
    /// if things are unterminated or if there's an invalid literal
    /// or with some garbage on the end of the number
    pub fn lex_all_tokens(mut self) -> Result<Vec<Token<FileName>>, LexError> {
        let mut tokens = Vec::new();

        loop {
            {
                let token = self.next_token()?;

                match token.kind {
                    TokenKind::Eof => {
                        tokens.push(token);
                        break;
                    }
                    _ => tokens.push(token),
                }
            }
        }

        Ok(tokens)
    }

    /// Called while the current lexer mode is `Normal`
    ///
    /// Attempts to lex the next bit of input content as a `Token` depending
    /// on the current mode the lexer is in (any but `StrLit`).
    ///
    /// This may also change the mode stack of the `Lexer` depending
    /// on whether or we are in a `StrInterp`, `Quote` or `Splice`.
    ///
    /// Depth tracking is also done through `Brace`/`Paren` modes that are more
    /// structural than an anctual mode
    #[allow(clippy::too_many_lines)]
    fn lex_normal_token(&mut self) -> LexResult<FileName> {
        // Ignore whitespace
        self.skip_whitespace_comments();
        let start = self.current_pos();

        // check if we hit the end?
        let Some(c) = self.peek_char() else {
            return self.lex_eof();
        };

        // Handle potential newlines left over
        //
        // We need these for the parser.
        if c == '\n' {
            self.advance();
            return Ok(self.make_located(TokenKind::Newline, self.span_from(start)));
        }

        // First we need to handle everything that can
        // change the current mode before we lex normal tokens.

        // Quotes for macros
        if self.try_advance_str("``") {
            // Handle starting new quote or ending quote
            if matches!(self.current_mode(), LexerMode::Quote { start: _ }) {
                self.pop_mode();
                return Ok(self.make_located(TokenKind::MacroQuoteEnd, self.span_from(start)));
            }
            self.push_mode(LexerMode::Quote { start });
            return Ok(self.make_located(TokenKind::MacroQuoteStart, self.span_from(start)));
        }

        // Splices for macros
        if self.try_advance_str("$(") {
            // The close token is a normal `)`
            // where we need to handle Paren/Splice
            self.push_mode(LexerMode::Splice { start });
            return Ok(self.make_located(TokenKind::MacroSpliceStart, self.span_from(start)));
        }

        // NOTE:
        // for any multi-character sequence we must lex the longest
        // first before the shortest, e.g spread before slice before access/.

        if self.try_advance_str("=>") {
            return Ok(self.make_located(TokenKind::FatArrow, self.span_from(start)));
        }

        if self.try_advance_str("->") {
            return Ok(self.make_located(TokenKind::ThinArrow, self.span_from(start)));
        }

        if self.try_advance_str("...") {
            return Ok(self.make_located(TokenKind::DotDotDot, self.span_from(start)));
        }

        if self.try_advance_str("..") {
            return Ok(self.make_located(TokenKind::DotDot, self.span_from(start)));
        }

        if self.try_advance_str("|>") {
            return Ok(self.make_located(TokenKind::PipeArrow, self.span_from(start)));
        }

        // It's just kinda nicer for the more traditional operators if we
        // forcibly reserve them and then make them identifiers here, to prevent
        // gluing for them as programmers are used to being able to glue em to idents
        if self.try_advance_str("**") {
            return Ok(self.make_located(TokenKind::Ident(String::from("**")), self.span_from(start)));
        }

        if self.try_advance_str("==") {
            return Ok(self.make_located(TokenKind::Ident(String::from("==")), self.span_from(start)));
        }

        if self.try_advance_str("!=") {
            return Ok(self.make_located(TokenKind::Ident(String::from("!=")), self.span_from(start)));
        }

        if self.try_advance_str("&&") {
            return Ok(self.make_located(TokenKind::Ident(String::from("&&")), self.span_from(start)));
        }

        if self.try_advance_str("||") {
            return Ok(self.make_located(TokenKind::Ident(String::from("||")), self.span_from(start)));
        }

        if self.try_advance_str("<=") {
            return Ok(self.make_located(TokenKind::Ident(String::from("<=")), self.span_from(start)));
        }

        if self.try_advance_str(">=") {
            return Ok(self.make_located(TokenKind::Ident(String::from(">=")), self.span_from(start)));
        }

        // Check if we are entering a StrLiteral section.
        if self.try_advance_str("\"") {
            self.push_mode(LexerMode::StrTextLiteral { start });
            return Ok(self.make_located(TokenKind::StrStart, self.span_from(start)));
        }

        // Try lex numbers now since "." falls in normal punctuation (lexxed below)
        // but we want to try grab numbers first
        //
        // See comment of `lex_number` for required conditions for being maybe a number.
        if self.peek_char().is_some_and(|c| c.is_ascii_digit())
            || (self.peek_char() == Some('.')
                && self.peek_char_nth(1).is_some_and(|d| d.is_ascii_digit()))
        {
            return self.lex_number();
        }

        if c == '@' {
            self.advance();
            return Ok(self.make_located(TokenKind::MacroAt, self.span_from(start)));
        }
        if c == '#' {
            self.advance();
            return Ok(self.make_located(TokenKind::MacroHash, self.span_from(start)));
        }
        if c == '=' {
            self.advance();
            return Ok(self.make_located(TokenKind::Equal, self.span_from(start)));
        }
        if c == '.' {
            self.advance();
            return Ok(self.make_located(TokenKind::Dot, self.span_from(start)));
        }
        if c == ',' {
            self.advance();
            return Ok(self.make_located(TokenKind::Comma, self.span_from(start)));
        }
        if c == ':' {
            self.advance();
            return Ok(self.make_located(TokenKind::Colon, self.span_from(start)));
        }
        if c == ';' {
            self.advance();
            return Ok(self.make_located(TokenKind::Semicolon, self.span_from(start)));
        }
        if c == '[' {
            self.advance();
            return Ok(self.make_located(TokenKind::DelLBracket, self.span_from(start)));
        }
        if c == ']' {
            self.advance();
            return Ok(self.make_located(TokenKind::DelRBracket, self.span_from(start)));
        }
        if c == '<' {
            self.advance();
            return Ok(self.make_located(TokenKind::DelLAngle, self.span_from(start)));
        }
        if c == '>' {
            self.advance();
            return Ok(self.make_located(TokenKind::DelRAngle, self.span_from(start)));
        }

        // Simple operators once again for gluing...
        if c == '+' {
            self.advance();
            return Ok(self.make_located(TokenKind::Ident(String::from("+")), self.span_from(start)));
        }

        if c == '-' {
            self.advance();
            return Ok(self.make_located(TokenKind::Ident(String::from("-")), self.span_from(start)));
        }

        if c == '*' {
            self.advance();
            return Ok(self.make_located(TokenKind::Ident(String::from("*")), self.span_from(start)));
        }

        if c == '/' {
            self.advance();
            return Ok(self.make_located(TokenKind::Ident(String::from("/")), self.span_from(start)));
        }

        if c == '!' {
            self.advance();
            return Ok(self.make_located(TokenKind::Ident(String::from("!")), self.span_from(start)));
        }

        if c == '&' {
            self.advance();
            return Ok(self.make_located(TokenKind::Ident(String::from("&")), self.span_from(start)));
        }

        if c == '|' {
            self.advance();
            return Ok(self.make_located(TokenKind::Ident(String::from("|")), self.span_from(start)));
        }

        if c == '%' {
            self.advance();
            return Ok(self.make_located(TokenKind::Ident(String::from("%")), self.span_from(start)));
        }

        if c == '^' {
            self.advance();
            return Ok(self.make_located(TokenKind::Ident(String::from("^")), self.span_from(start)));
        }

        if c == '~' {
            self.advance();
            return Ok(self.make_located(TokenKind::Ident(String::from("~")), self.span_from(start)));
        }

        // Delimiters (need mode structural tracking/handling Interp/Slice)

        // Open { is handled in `StrTextLiteral` mode.
        if c == '{' {
            self.advance();
            self.push_mode(LexerMode::Brace { start });
            return Ok(self.make_located(TokenKind::DelLBrace, self.span_from(start)));
        }
        if c == '}' {
            self.advance();
            match self.current_mode() {
                // Close either the structural tracking or StrInterp
                LexerMode::StrInterp { start: _ } => {
                    self.pop_mode();
                    return Ok(self.make_located(TokenKind::StrInterpEnd, self.span_from(start)));
                }
                LexerMode::Brace { start: _ } => {
                    self.pop_mode();
                    return Ok(self.make_located(TokenKind::DelRBrace, self.span_from(start)));
                }

                // Doesn't close anything... weird
                _ => {
                    return Err(LexError::UnmatchedRBrace {
                        at: self.span_from(start),
                    });
                }
            }
        }

        // same for parenthesis....
        if c == '(' {
            self.advance();
            self.push_mode(LexerMode::Paren { start });
            return Ok(self.make_located(TokenKind::DelLParen, self.span_from(start)));
        }
        if c == ')' {
            self.advance();
            match self.current_mode() {
                // Close either the structural tracking or Splice
                LexerMode::Splice { start: _ } => {
                    self.pop_mode();
                    return Ok(self.make_located(TokenKind::MacroSpliceEnd, self.span_from(start)));
                }
                LexerMode::Paren { start: _ } => {
                    self.pop_mode();
                    return Ok(self.make_located(TokenKind::DelRParen, self.span_from(start)));
                }

                // Doesn't close anything... weird
                _ => {
                    return Err(LexError::UnmatchedRParen {
                        at: self.span_from(start),
                    });
                }
            }
        }

        // Since whitespace, delimiters, macros, strings, and numbers are handled,
        // above, whatever remains is our idents, keywords or booleans
        let mut text = String::new();
        while let Some(next_c) = self.peek_char() {
            // A reserved character indicates we are at the boundary of our current token
            if next_c.is_whitespace() || Self::is_reserved(next_c) {
                break;
            }

            // build the ident/kw/bool
            text.push(
                self.advance()
                    .expect("peek_char returned some for next char"),
            );
        }

        Ok(self.make_located(
            Self::lex_ident_or_keyword_or_boolean(text),
            self.span_from(start),
        ))
    }

    /// Returns either a `Ok(EOF)` when we can safely
    /// end the lexing/file here.
    ///
    /// Or an `Err(LexError)` when we cant, maybe on an
    /// unresolved Interp or something similar
    fn lex_eof(&mut self) -> LexResult<FileName> {
        let start = self.current_pos();

        match self.modes.last() {
            Some(mode) => match mode {
                // At the root/normal mode, not nested anywhere, all okay :)
                LexerMode::Normal => Ok(self.make_located(TokenKind::Eof, self.span_from(start))),

                // Unterminated string literal, shouldnt reach this branch
                // but just in case.
                LexerMode::StrTextLiteral { start } => Err(LexError::UnterminatedString {
                    start: self.span_from(*start),
                }),

                // Other unterminated modoes
                LexerMode::StrInterp { start } => Err(LexError::UnterminatedInterp {
                    start: self.span_from(*start),
                }),
                LexerMode::Quote { start } => Err(LexError::UnterminatedQuote {
                    start: self.span_from(*start),
                }),
                LexerMode::Splice { start } => Err(LexError::UnterminatedSplice {
                    start: self.span_from(*start),
                }),
                LexerMode::Brace { start } => Err(LexError::UnterminatedLBrace {
                    start: self.span_from(*start),
                }),
                LexerMode::Paren { start } => Err(LexError::UnterminatedLParen {
                    start: self.span_from(*start),
                }),
            },
            None => panic!("This should be impossible! Somewhere popped more modes than pushed"),
        }
    }

    /// Attempts to lex the text as a keyword or boolean, or otherwise
    /// returns the text as a `Token` of the kind `Ident`
    fn lex_ident_or_keyword_or_boolean(text: String) -> TokenKind {
        match text.as_str() {
            "fn" => TokenKind::KwFn,
            "macro" => TokenKind::KwMacro,
            "let" => TokenKind::KwLet,
            "mut" => TokenKind::KwMut,
            "type" => TokenKind::KwType,
            "object" => TokenKind::KwObject,
            "enum" => TokenKind::KwEnum,
            "match" => TokenKind::KwMatch,
            "if" => TokenKind::KwIf,
            "else" => TokenKind::KwElse,
            "for" => TokenKind::KwFor,
            "while" => TokenKind::KwWhile,
            "loop" => TokenKind::KwLoop,
            "break" => TokenKind::KwBreak,
            "continue" => TokenKind::KwContinue,
            "return" => TokenKind::KwReturn,
            "raise" => TokenKind::KwRaise,
            "try" => TokenKind::KwTry,
            "catch" => TokenKind::KwCatch,
            "import" => TokenKind::KwImport,
            "pub" => TokenKind::KwPub,
            "as" => TokenKind::KwAs,
            "is" => TokenKind::KwIs,
            "in" => TokenKind::KwIn,
            "with" => TokenKind::KwWith,
            "where" => TokenKind::KwWhere,
            "fail" => TokenKind::KwFail,
            "defer" => TokenKind::KwDefer,
            "true" => TokenKind::LitBool(true),
            "false" => TokenKind::LitBool(false),
            _ => TokenKind::Ident(text),
        }
    }

    /// Skips whitespace and comments
    /// from the current position in the `Lexer`'s cursor.
    fn skip_whitespace_comments(&mut self) {
        loop {
            match self.peek_char() {
                // new lines are a dedicated token
                Some(c) if c.is_whitespace() && c != '\n' => {
                    self.advance();
                }
                Some('/') if self.peek_char_nth(1) == Some('/') => {
                    // Advance until the next new line
                    while let Some(c) = self.peek_char() {
                        if c == '\n' {
                            break;
                        }
                        self.advance();
                    }
                }
                _ => break,
            }
        }
    }

    /// Called while the current lexer mode is `StrLit`
    ///
    /// Scans either a `StrFragment` of plain text with escapes resolved,
    /// or a boundary token such as a `StrInterpStart` on seeing `${`, or
    /// a `StrEnd` on seeing the closing `"`.
    ///
    /// After a boundary token, the mode is changed from `StrLit` to either
    /// `StrInterp` or back out of `StrLit` from popping the `StrLit` mode.
    fn lex_str_fragment_or_boundary(&mut self) -> LexResult<FileName> {
        // Start of this string fragment/boundary
        let start = self.current_pos();

        // Check boundaries first before we greedly
        // grab into a `StrFragment`

        // End of string
        if self.try_advance_str("\"") {
            self.pop_mode();
            return Ok(self.make_located(TokenKind::StrEnd, self.span_from(start)));
        }

        // Start of a StrInterp
        if self.try_advance_str("${") {
            self.push_mode(LexerMode::StrInterp { start });
            return Ok(self.make_located(TokenKind::StrInterpStart, self.span_from(start)));
        }

        // hit EOF before string terminated
        if self.peek_char().is_none() {
            return Err(LexError::UnterminatedString {
                start: self.span_from(start),
            });
        }

        // Greedly eat this as a `StrFragment`
        let mut text = String::new();

        loop {
            match self.peek_char() {
                None => {
                    return Err(LexError::UnterminatedString {
                        start: self.span_from(start),
                    });
                }

                // Boundary, let next call handle
                Some('"') => break,
                Some('$') if self.peek_char_nth(1) == Some('{') => break,

                // Escape sequence
                Some('\\') => {
                    let esc_start = self.current_pos();

                    // Consume the \ and then the actual escape sequence
                    self.advance();
                    match self.advance() {
                        Some('n') => text.push('\n'),
                        Some('t') => text.push('\t'),
                        Some('r') => text.push('\r'),
                        Some('\\') => text.push('\\'),
                        Some('"') => text.push('"'),
                        Some('$') => text.push('$'),
                        Some('0') => text.push('\0'),
                        Some(_) => {
                            return Err(LexError::InvalidEscape {
                                at: self.span_from(esc_start),
                            });
                        }
                        None => {
                            return Err(LexError::UnterminatedString {
                                start: self.span_from(start),
                            });
                        }
                    }
                }

                // Random other characters
                Some(other) => {
                    text.push(other);
                    self.advance();
                }
            }
        }

        Ok(self.make_located(TokenKind::StrFragment(text), self.span_from(start)))
    }

    /// Attempt to tokenise a numeric literal
    ///
    /// A number is where the current char is either an ASCII digit,
    /// or `.` followed by an ASCII digit.
    ///
    /// See `Fermion3` specification for actual language spec
    fn lex_number(&mut self) -> LexResult<FileName> {
        // Start of this number
        let start = self.current_pos();

        // If it begins with 0 followed by one of the base
        // "specifiers" then try lex that base
        if self.peek_char() == Some('0') {
            match self.peek_char_nth(1) {
                Some('x' | 'X') => return self.lex_base_int(2, 16),
                Some('b' | 'B') => return self.lex_base_int(2, 2),
                Some('o' | 'O') => return self.lex_base_int(2, 8),
                _ => {}
            }
        }

        // The text of the literal
        // we build this from here while we figure out if its
        // a float, UInt or Int for later parsing
        let mut text = String::new();
        let mut is_float = false;

        // Starts with "." so must be a float
        // such as: `.5`
        if self.peek_char() == Some('.') {
            is_float = true;
            text.push('.');
            self.advance();
            self.consume_base_10_digits(&mut text);
        } else {
            // No start with a "." so we check and consume the integer part
            self.consume_base_10_digits(&mut text);

            // Check whether a fractional part follows with a "." for a float
            if self.peek_char() == Some('.') && self.peek_char_nth(1) != Some('.') {
                is_float = true;
                text.push('.');
                self.advance();
                self.consume_base_10_digits(&mut text);
            }
        }

        // Exponent for a float
        if matches!(self.peek_char(), Some('e' | 'E')) {
            // Make a checkpoint here to grab all of exponent chars
            // if they exist (this is so we can revert the lexer Chars<>
            // if its not actually a valid exponent to let trailing garbage handle
            // the error)
            let exp_checkpoint = self.clone_chars();
            let mut exp_text = String::new();
            exp_text.push(
                self.advance()
                    .expect("expected 'e'/'E' to be here since peek_char returned"),
            );

            if matches!(self.peek_char(), Some('+' | '-')) {
                exp_text.push(
                    self.advance()
                        .expect("expected '+'/'-' to be here since peek_char returned it"),
                );
            }

            // Consume all of the exponent digits
            if self.peek_char().is_some_and(is_char_valid_base_10) {
                self.consume_base_10_digits(&mut exp_text);
                is_float = true;
                text.push_str(&exp_text);
            } else {
                // Not actually an exponent, let trailing
                // garbage handle error later.
                self.chars = exp_checkpoint;
            }
        }

        // Check for `UInt` suffix of `u`/`U` suffix
        // same case with the exponents and trailing garbage
        let has_u_suffix = self.peek_is_valid_uint_suffix();

        if has_u_suffix {
            // floats cant be unsigned...
            if is_float {
                let bad_start = self.current_pos();
                self.advance();
                return Err(LexError::MalformedNumber {
                    at: self.span_from(bad_start),
                    reason: String::from("float literals cannot have a `u` suffix"),
                });
            }
            self.advance();
        }

        // Prevent ident gluing onto the numeric literal when unexpected
        self.check_trailing_garbage(start, "invalid trailing characters after numeric literal")?;

        // Produce the actual token from the text we built up while advancing
        if has_u_suffix {
            let value: u64 = text.parse().unwrap_or(0);
            return Ok(self.make_located(TokenKind::LitUInt(value), self.span_from(start)));
        }

        if is_float {
            let value: f64 = text.parse().unwrap_or(f64::NAN);
            Ok(self.make_located(TokenKind::LitFloat(value), self.span_from(start)))
        } else {
            let value: i64 = text.parse().unwrap_or(0);
            Ok(self.make_located(TokenKind::LitInt(value), self.span_from(start)))
        }
    }

    /// Consumes a run of base-10 digits at the current position,
    /// appending each digit to `buf`
    ///
    /// `_` is accepted as a visual separator (e.g. `1_000_000`)
    /// but is not part of the final `buf`.
    fn consume_base_10_digits(&mut self, buf: &mut String) {
        while let Some(c) = self.peek_char() {
            // ignore `_``
            if c == '_' {
                self.advance();

            // push other valid base 10 digits
            // `_` is eaten by above
            } else if is_char_valid_base_10(c) {
                buf.push(c);
                self.advance();
            } else {
                break;
            }
        }
    }

    /// Lexes some `Int` or `UInt` at the current position
    ///
    /// `prefix_len` is the number of elements to skip in the prefix as part
    /// of this base, for example `0x` has 2 elements in the prefix before
    /// the actual `Int`/`UInt` starts.
    ///
    /// `base` is the base of the integer.
    fn lex_base_int(&mut self, prefix_len: usize, base: u32) -> LexResult<FileName> {
        // Grab the current start position for errors that start from here
        let start = self.current_pos();

        // Ignore the prefix
        for _ in 0..prefix_len {
            self.advance();
        }

        // The position at which digits start..
        let digits_start = self.current_pos();

        // Yoink all the digits part of the int. `_` is allowed as a
        // visual separator for long Ints/UInts and is dropped here
        // rather than collected, so `digits` is parse-ready as-is.
        let mut digits = String::new();
        while let Some(c) = self.peek_char() {
            if c == '_' {
                self.advance();
            } else if c.is_digit(base) {
                digits.push(c);
                self.advance();
            } else {
                break;
            }
        }

        // No digits since current = start for digits, which is illegal!
        if self.current_pos() == digits_start {
            return Err(LexError::MalformedNumber {
                at: self.span_from(start),
                reason: String::from("expected digits after base prefix such as: `0x`, `0b`, `0o`"),
            });
        }

        // Consume `UInt` suffix if it has one
        let has_uint_suffix = self.peek_is_valid_uint_suffix();
        if has_uint_suffix {
            self.advance();
        }

        // Ensure there's no trailing garbage to prevent accidental ident gluing
        self.check_trailing_garbage(start, "invalid trailing characters after radix literal")?;

        if has_uint_suffix {
            let value = u64::from_str_radix(&digits, base).unwrap_or(0);
            Ok(self.make_located(TokenKind::LitUInt(value), self.span_from(start)))
        } else {
            let value = i64::from_str_radix(&digits, base).unwrap_or(0);
            Ok(self.make_located(TokenKind::LitInt(value), self.span_from(start)))
        }
    }

    /// Returns whether or not the next character is a valid unsigned
    /// int suffix for an integer (basically is the next `u` or `U`)
    fn peek_is_valid_uint_suffix(&self) -> bool {
        matches!(self.peek_char(), Some('u' | 'U'))
    }

    /// Check that there is no trailing garbage (non-reserved/whitespace)
    /// following this number
    ///
    /// Returns Ok(()) if there is none else a `LexError`.
    fn check_trailing_garbage(
        &mut self,
        start: usize,
        reason: &'static str,
    ) -> Result<(), LexError> {
        if self
            .peek_char()
            .is_some_and(|c| !c.is_whitespace() && !Self::is_reserved(c))
        {
            // Consume the rest of the unspaced characters so the error span is perfectly sized
            while self
                .peek_char()
                .is_some_and(|c| !c.is_whitespace() && !Self::is_reserved(c))
            {
                self.advance();
            }

            return Err(LexError::MalformedNumber {
                at: self.span_from(start),
                reason: String::from(reason),
            });
        }

        Ok(())
    }
}

/// Returns whether or not a character is a valid part of a digit
/// under the provided base. `_` is always accepted as a visual
/// separator between digits for long numbers, e.g. `1_000_000`.
///
/// NOTE: If supporting any new bases beyond 16, this will need updating.
fn is_char_valid_under_base(c: char, base: u32) -> bool {
    c.is_digit(base) || c == '_'
}

/// Returns whether or not a character is a valid part of a digit
/// in base 10 (including the `_` separator).
fn is_char_valid_base_10(c: char) -> bool {
    is_char_valid_under_base(c, 10)
}
