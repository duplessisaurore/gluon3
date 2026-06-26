//! The binding resolvers path simplification trait
//! 
//! This is because the binding resolver is inherently
//! file system/no_std agnostic, and we cannot
//! really expect file paths to be standardised, so we expect this trait
//! to do all the simplification to an actual identifier for us

use alloc::string::String;

/// A trait which specifies some type is a "path simplifier" which takes
/// in our already valid path name and turns it into a simple identifier
pub trait PathSimplifier {
    type PathSimplificationError;

    /// Resolve a `String` identifier from the path
    /// 
    /// This should not validate the file exists, but instead only
    /// take the file path and simplify it down to an identifier for things
    /// to refer to this path's module with
    fn simplify_path_to_ident<'path>(&mut self, path: &'path str) -> Result<String, Self::PathSimplificationError>;
}
