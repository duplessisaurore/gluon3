//! Errors that can occur during the module resolving process

use core::fmt::Display;

use alloc::{string::String, vec::Vec};
use gluon_debug::{Located, SourceFile, SourceLocation};
use gluon_lexer::LexError;
use gluon_parser::{ast::AstNode, errors::ParseError};

/// Result of one step of the module resolving process
pub type ModuleResolveResult<T, FileName, PathResolveError> = Result<T, ModuleResolveError<FileName, PathResolveError>>;

/// Errors that can occur while module resolving.
#[derive(Debug, Clone, PartialEq)]
pub enum ModuleResolveError<FileName: Display + Clone + PartialEq, PathResolveError> {
    /// This module could not be found
    /// on the system using the provided loader
    ModuleNotFound {
        path: SourceFile<FileName>,
        wanted_by: SourceLocation<FileName>
    },

    /// A cyclic dependency was detected in the import graph
    /// 
    /// This is such as:
    /// Module A imports B -> Module B imports A -> ?? Cycle of doom!!
    /// 
    /// `path` is the module paths which are resolved to a cyclic import
    CyclicDependencies {
        cyclic_path: String,
    },

    /// The sourced dependency module failed lexing
    LexerError {
        error: Located<LexError, FileName>
    },

    /// The sourced dependency module failed parsing
    ParserError {
        errors: Vec<Located<ParseError, FileName>>
    },

    /// There was an unexpected non-import expression in the 
    /// imports section of a `Module`
    UnexpectedNonImport {
        found: AstNode<FileName>
    },

    /// Some error to do with module resolving of a path
    ModulePathResolveError {
        error: PathResolveError
    }
}
