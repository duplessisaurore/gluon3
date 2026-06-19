//! The actual `Lexer` type
//! 
//! This translates the source file content into the tokens
//! specified by `TokenKind` for one source file.

use alloc::rc::Rc;
use gluon_debug::{Located, SourceFile, SourceLocation, Span};

/// The actual lexer class itself
/// 
/// The mode that the lexer is in is stored in
/// a stack as we can have nested string interpolation to
/// track the brackets well
pub struct Lexer<'a> {
    /// The source file content we are currently lexing
    source: &'a str,

    /// The source file to track from which the contents
    /// to lex came from
    file: Rc<SourceFile>
}

impl<'a> Lexer<'a> {
    /// Returns a new Located<T> for the kind with a source span in the current
    /// stored file of the `Lexer`.
    fn make_located<T: Clone>(&self, kind: T, source_span: Span) -> Located<T> {
        Located {
            kind,
            location: SourceLocation {
                file: Rc::clone(&self.file),
                span: source_span,
            },
        }
    }
}

