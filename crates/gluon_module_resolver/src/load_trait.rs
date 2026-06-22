//! The module loaders path resolution trait
//! 
//! This is because the module loader is inherently
//! file system/no_std agnostic, and we cannot
//! really load things ourselves, so we expect this trait
//! to do all the loading for us and return the source content

use core::fmt::{Display};

use alloc::string::String;
use gluon_debug::SourceFile;

/// A trait which specifies some type is a "module loader" which takes
/// in our file names and turns them into the textual content for our lexer + parser
/// to parse into another module
pub trait LoadModule<FileName: Display + Clone + PartialEq> {
    type ResolveSourceError;

    /// Resolve a SourceFile from the path
    /// 
    /// This should not validate the file exists, but instead only
    /// resolve it in the context that modules are being loaded
    fn resolve_source_file<'path>(&mut self, path: &'path str) -> Result<SourceFile<FileName>, Self::ResolveSourceError>;

    /// Load a module from its SourceFile
    /// 
    /// This should return the source the string 
    /// textual content if the path exists, otherwise a None
    fn load_module_from_path<'path>(&mut self, path: &'path SourceFile<FileName>) -> Option<String>;
}
