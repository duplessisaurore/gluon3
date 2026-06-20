//! `Gluon3` is an experimental free and open-source compiler for the `Fermion3`
//! language that translates `Fermion3` into the more textual assembly language `Quark3`.
//!
//! Check out the [repository README](https://github.com/duplessisaurore/gluon3/blob/main/README.md)
//! for more information about the project and join the [Discord](https://discord.gg/wXzj2cqZ3Q) for
//! any discussion.
//!
//! ## Gluon3 Debug
//!
//! The `debug` crate provides the set of debugging structs and helpers
//! for shared debugging capabilities between different phases of the `Gluon3`
//! compiler

#![warn(clippy::pedantic)]
#![no_std]

use core::fmt::Display;

use alloc::{
    rc::Rc,
    string::{String, ToString},
};

extern crate alloc;

/// A source file which points to some file on disk
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile<FileName: Display> {
    pub filename: FileName,
}

/// A source location in a file
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLocation<FileName: Display> {
    pub file: Rc<SourceFile<FileName>>,
    pub span: Span,
}

impl<FileName: Display> SourceLocation<FileName> {
    #[must_use]
    pub fn new(file: Rc<SourceFile<FileName>>, span: Span) -> Self {
        Self { file, span }
    }

    #[must_use]
    pub fn filename(&self) -> String {
        self.file.filename.to_string()
    }
}

/// Attach some location information onto a type
#[derive(Debug, Clone)]
pub struct Located<T: Clone, FileName: Display> {
    pub kind: T,
    pub location: SourceLocation<FileName>,
}

/// Span of byte offsets into the source file that
/// a token originated from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl<FileName: Display> SourceLocation<FileName> {
    #[must_use]
    pub fn new_span_in_file(&self, span: Span) -> Self {
        Self {
            file: self.file.clone(),
            span,
        }
    }
}

impl Span {
    #[must_use]
    pub fn join(self, other: Span) -> Span {
        Span {
            start: self.start,
            end: other.end,
        }
    }
}

impl<T: Clone, FileName: Display> Located<T, FileName> {
    pub fn span(&self) -> Span {
        self.location.span
    }
}
