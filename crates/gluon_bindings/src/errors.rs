//! Errors that can occur during the binding resolving process

use alloc::string::String;
use gluon_debug::Located ;

/// Result of one step of the binding resolving process
pub type BindingResolveResult<T, FileName> = Result<T, BindingResolveError<FileName>>;

/// A located binding resolve error
pub type BindingResolveError<FileName> = Located<BindingResolveErrorInner, FileName>;

/// An error that can occur while binding resolving
/// the AST.
#[derive(Debug, Clone, PartialEq)]
pub struct BindingResolveErrorInner {
    kind: BindingResolveErrorKind,
    name: String
}

/// All the inner kinds for errors that can occur while binding resolving.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BindingResolveErrorKind {
    /// Encountered a name at a location that was unresolved
    /// 
    /// This means a binding did not exist for it yet.
    UnresolvedName,

    /// A duplicate top level definition of a name was found
    DuplicateTopLevelDefinition,

    /// There was an attempted assignment to an immutable binding
    AssignmentToImmutable,

    /// There was an attempted assignment to a non-local binding
    AssignmentToNonLocal,
}