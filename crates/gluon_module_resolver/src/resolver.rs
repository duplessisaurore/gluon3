//! The actual `ModuleResolver` type
//!
//! This takes an initial module `Module` an d attempts
//! to manage the dependency chain of the modules so
//! that all dependencies are loaded.

use core::{
    fmt::{Debug, Display},
    hash::Hash,
};

use alloc::{format, rc::Rc, string::ToString, vec::Vec};
use gluon_debug::SourceFile;
use gluon_lexer::Lexer;
use gluon_parser::{
    Parser,
    ast::{ExprKind, Module},
};
use hashbrown::HashMap;

use crate::{LoadModule, ModuleResolveError, errors::ModuleResolveResult};

/// The fully resolved dependency graph output of the `ModuleLoader`
#[derive(Debug)]
pub struct ResolvedGraph<FileName: Display + Clone + PartialEq + Hash> {
    /// This is a mapping of the name of the file
    /// to its actual `Module` which has been loaded and parsed.
    modules: HashMap<Rc<SourceFile<FileName>>, Module<FileName>>,
}

/// The actual module loader type itself
///
/// The loader is responsible for actually finding the files
/// and getting their source contents.
pub struct ModuleLoader<FileName: Display + Clone + PartialEq + Debug, Loader: LoadModule<FileName>>
{
    /// The initial module from which all dependencies are
    /// branching out of, as a binary can only have one entry point
    module: Module<FileName>,

    /// The loader to find all other files and their contents
    loader: Loader,
}

impl<FileName: Display + Clone + PartialEq + Hash + Eq + Debug, Loader: LoadModule<FileName>>
    ModuleLoader<FileName, Loader>
{
    /// Create a new module loader over `module` that will parse all of the
    /// imports and depdenencies into a singular `ResolvedGraph` for this
    pub fn new(module: Module<FileName>, loader: Loader) -> Self {
        Self { module, loader }
    }

    /// Resolves all the modules starting from the source file
    /// with this `ModuleLoader` using the supplied loader.
    ///
    /// # Errors
    ///
    /// If during any module loading there was an error in lexing or
    /// parsing, this will error.
    ///
    /// If the file could not be loaded, this will also error or
    /// if the dependencies of the modules are cyclic.
    pub fn resolve_modules(
        self,
    ) -> ModuleResolveResult<ResolvedGraph<FileName>, FileName, Loader::ResolveSourceError> {
        // All resolved modules will be added to this `ResolvedGraph`
        let mut resolved_graph = ResolvedGraph {
            modules: HashMap::new(),
        };

        // Things which are not yet quite resolved/in progress
        let mut in_progress = Vec::new();

        // The loader, we move this out to pass it to our walker
        // of imports
        let mut loader = self.loader;

        // The start with walking the imports from the root module/initial module
        Self::walk_imports(
            &self.module,
            &mut loader,
            &mut resolved_graph,
            &mut in_progress,
        )?;

        // At the very end the root module is also part of the resolved graph
        resolved_graph
            .modules
            .insert(Rc::clone(&self.module.name), self.module);

        Ok(resolved_graph)
    }

    fn walk_imports(
        current_module: &Module<FileName>,
        loader: &mut Loader,
        resolved_modules: &mut ResolvedGraph<FileName>,
        in_progress: &mut Vec<Rc<SourceFile<FileName>>>,
    ) -> ModuleResolveResult<(), FileName, Loader::ResolveSourceError> {
        // Walk each import in the module..
        for import in &current_module.imports {
            // Grab the import path, we enforce parser correctness here because why not
            let import_path = match &import.kind {
                ExprKind::Import { path, alias: _ } => path,
                _ => {
                    return Err(ModuleResolveError::UnexpectedNonImport {
                        found: import.clone(),
                    });
                }
            };

            // If its already been resolved then ignore
            let path = Rc::new(
                loader
                    .resolve_source_file(import_path)
                    .map_err(|error| ModuleResolveError::ModulePathResolveError { error })?
            );
            if resolved_modules.modules.contains_key(&path) {
                continue;
            };

            // If its in "in progress" then we have a cyclic import!
            if in_progress.contains(&path) {
                // Build the full error path -> chain
                let mut error_path = in_progress
                    .first()
                    .expect("checked that in_progress at least contains &path")
                    .filename
                    .to_string();
                for path in &in_progress[1..] {
                    error_path = format!("{error_path} -> {}", path.filename);
                }

                return Err(ModuleResolveError::CyclicDependencies {
                    cyclic_path: error_path,
                });
            };

            // Load and parse the module, otherwise error if not found.
            let source = loader.load_module_from_path(&path).ok_or_else(|| {
                ModuleResolveError::ModuleNotFound {
                    path: Rc::as_ref(&path).clone(),
                    wanted_by: import.location.clone(),
                }
            })?;

            // Lex the source
            let lexer = Lexer::new(&source, Rc::clone(&path));
            let tokens = lexer
                .lex_all_tokens()
                .map_err(|error| ModuleResolveError::LexerError { error })?;

            // Parse the source into its module..
            let mut parser = Parser::new(tokens, Rc::clone(&path));
            let loaded_module = parser
                .parse_module()
                .map_err(|error| ModuleResolveError::ParserError { error })?;

            // This is now in progress of resolving its dependencies
            in_progress.push(Rc::clone(&path));
            Self::walk_imports(&loaded_module, loader, resolved_modules, in_progress)?;

            // Done walking its imports, remove from `in_progress`
            in_progress.pop();

            // Add to modules map
            resolved_modules.modules.insert(path, loaded_module);
        }

        Ok(())
    }
}
