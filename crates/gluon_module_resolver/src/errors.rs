//! Errors that can occur during the module resolving process

use core::fmt::Display;

use alloc::string::String;
use gluon_debug::{Located};
use gluon_lexer::LexError;
use gluon_parser::errors::ParseError;

/// Result of one step of the module resolving process
pub type ModuleResolveResult<T, FileName> = Result<T, ModuleResolveError<FileName>>;

/// Errors that can occur while module resolving.
#[derive(Debug, Clone, PartialEq)]
pub enum ModuleResolveError<FileName: Display + Clone + PartialEq> {
    /// This module could not be found
    /// on the system using the provided loader
    ModuleNotFound {
        path: Located<String, FileName>
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
        error: Located<ParseError, FileName>
    }
}
