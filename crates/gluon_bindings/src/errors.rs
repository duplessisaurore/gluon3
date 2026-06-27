//! Errors that can occur during the binding resolving process

use core::fmt::Display;

use alloc::{rc::Rc, string::String, vec::Vec};
use gluon_debug::{Located, SourceFile, SourceLocation} ;
use gluon_parser::ast::ExprKind;

/// Result of one step of the binding resolving process
pub type BindingResolveResult<T, FileName, PathSimplificationError, ResolveSourceError> = Result<T, BindingResolveError<FileName, PathSimplificationError, ResolveSourceError>>;

/// A located binding resolve error
pub type BindingResolveError<FileName, PathSimplificationError, ResolveSourceError> = Located<BindingResolveErrorKind<FileName, PathSimplificationError, ResolveSourceError>, FileName>;

/// All the kinds of errors that can occur while binding resolving.
#[derive(Debug, PartialEq)]
pub enum BindingResolveErrorKind<FileName: Display + Clone + PartialEq, PathSimplificationError, ResolveSourceError> {
    /// Encountered a name at a location that was unresolved
    /// 
    /// This means a binding did not exist for it yet.
    UnresolvedName {
        name: String,
    },

    /// A duplicate top level definition of a name was found
    /// to be defined originally at `original`
    DuplicateTopLevelDefinition {
        name: String,
        original: SourceLocation<FileName>
    },

    /// There was an attempted assignment to an immutable binding
    /// 
    /// The immutable binding was defined originally at `original`
    AssignmentToImmutable {
        name: String,
        original: SourceLocation<FileName>
    },

    /// There was an attempted assignment to a non-local binding
    /// 
    /// The binding was defined originally at `original`
    AssignmentToNonLocal {
        name: String,
        original: SourceLocation<FileName>
    },

    /// An unexpected AstNode occured here with some kind
    UnexpectedExprKind {
        expected: String,
        kind: ExprKind<FileName>
    },

    /// An error occured when trying to simplify a path
    PathSimplificationError {
        path: String,
        error: PathSimplificationError
    },

    /// An error occured when trying to resolve a path
    PathResolveError {
        error: ResolveSourceError
    }
}

/// Errors that can occur during the cross-module
/// resolution process
#[derive(Debug)]
pub enum CrossModuleError<FileName, PathErr, ResolveErr>
where
    FileName: Display + Clone + PartialEq,
{
    /// Binding resolution failed for an individual module.
    PerModuleErrors {
        source_file: Rc<SourceFile<FileName>>,
        errors: Vec<BindingResolveError<FileName, PathErr, ResolveErr>>,
    },

    /// When trying to resolve the `module.field` access
    /// 
    /// There does not exist such an export in the target module
    NoSuchExport {
        location: SourceLocation<FileName>,
        module_path: Rc<SourceFile<FileName>>,
        field: String,
    },

    /// When trying to resolve the `module.field` access
    /// 
    /// The item exists but is private!
    PrivateExport {
        location: SourceLocation<FileName>,
        module_path: Rc<SourceFile<FileName>>,
        field: String,
    },
}