//! The actual `ModuleResolver` type
//!
//! This takes an initial module `Module` an d attempts
//! to manage the dependency chain of the modules so
//! that all dependencies are loaded.

use core::fmt::Display;

use alloc::rc::Rc;
use gluon_debug::SourceFile;
use gluon_parser::ast::Module;
use hashbrown::HashMap;

use crate::LoadModule;

/// The fully resolved dependency graph output of the `ModuleLoader`
#[derive(Debug)]
pub struct ResolvedGraph<FileName: Display + Clone + PartialEq> {
    /// This is a mapping of the name of the file
    /// to its actual `Module` which has been loaded and parsed.
    modules: HashMap<Rc<SourceFile<FileName>>, Module<FileName>> 
}

/// The actual module loader type itself
pub struct ModuleLoader<FileName: Display + Clone + PartialEq, Loader: LoadModule<FileName>> {
    /// The initial module from which all dependencies are
    /// branching out of, as a binary can only have one entry point
    module: Module<FileName>,

    /// The loader to find all other files and their contents
    loader: Loader
}

impl<FileName: Display + Clone + PartialEq, Loader: LoadModule<FileName>> ModuleLoader<FileName, Loader> {
    /// Create a new module loader over `module` that will parse all of the
    /// imports and depdenencies into a singular `ResolvedGraph` for this
    pub fn new(module: Module<FileName>, loader: Loader) -> Self {
        Self {
            module,
            loader
        }
    }

    
}
