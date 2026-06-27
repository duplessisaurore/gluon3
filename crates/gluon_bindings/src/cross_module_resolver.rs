//! The cross-module resolver
//!
//! This runs the `BindingResolver` on each module
//! and validates their cross-module pending resolutions

use core::{fmt::Display, hash::Hash};

use alloc::{rc::Rc, vec::Vec};
use gluon_debug::{SourceFile, SourceLocation};
use gluon_module_resolver::{LoadModule, resolver::ResolvedGraph};
use gluon_parser::ast::{NodeId, Publicity};
use hashbrown::HashMap;

use crate::{
    binding_trait::PathSimplifier, bindings::BindingKind, errors::CrossModuleError, resolver::{BindingResolutionMap, BindingResolver, ModuleFieldResolution},
};

/// The fully resolved cross module included resolution map
///
/// Each module has it's own `BindingResolutionMap`
#[derive(Debug)]
pub struct CrossModuleResolutionMap<FileName: Display + Clone + PartialEq + Hash + Eq> {
    pub modules: HashMap<Rc<SourceFile<FileName>>, BindingResolutionMap<FileName>>,
}

/// The actual cross module binding resolver.
///
/// This will create a `CrossModuleResolutionMap`
/// for all of the modules part of the `graph` with
/// cross module resolutions done in a postpass
#[derive(Debug)]
pub struct CrossModuleBindingResolver<
    'graph,
    'loader,
    'simplifier,
    FileName: Display + Clone + PartialEq + Hash + Eq,
    PS: PathSimplifier,
    Loader: LoadModule<FileName>,
> {
    graph: &'graph ResolvedGraph<FileName>,
    path_simplifier: &'simplifier mut PS,
    loader: &'loader mut Loader,
}

impl<
    'graph,
    'loader,
    'simplifier,
    FileName: Display + Clone + PartialEq + Hash + Eq,
    PS: PathSimplifier,
    Loader: LoadModule<FileName>,
> CrossModuleBindingResolver<'graph, 'loader, 'simplifier, FileName, PS, Loader>
{
    /// Create a new binding resolver over `graph` that will resolve all of the
    /// bindings for all the modules into a singular `CrossModuleResolutionMap`
    pub fn new(
        graph: &'graph ResolvedGraph<FileName>,
        path_simplifier: &'simplifier mut PS,
        loader: &'loader mut Loader,
    ) -> Self {
        Self {
            graph,
            path_simplifier,
            loader,
        }
    }

    /// Runs the cross-module binding resolver to completion.
    ///
    /// This first runs the `BindingResolver` per module, creating each modules
    /// `BindingResolutionMap` with pending `Import` field accesses to be resolved.
    ///
    /// Then a second post-pass is done if all modules succeed phase 1, this takes
    /// all the pending import accesses and resolves them, ensuring they're valid.
    ///
    /// # Errors
    ///
    /// Returns all accumulated errors from either the resolver for a certain module,
    /// or if there was an issue that occured during cross-module resolving.
    pub fn resolve_all(
        self,
    ) -> Result<
        CrossModuleResolutionMap<FileName>,
        Vec<CrossModuleError<FileName, PS::PathSimplificationError, Loader::ResolveSourceError>>,
    > {
        // We destructure ourselves such that each element can be individiaully borrowed
        let Self {
            graph,
            path_simplifier,
            loader,
        } = self;

        // Store all errors and resolved module maps here
        let mut errors = Vec::new();
        let mut modules: HashMap<Rc<SourceFile<FileName>>, BindingResolutionMap<FileName>> =
            HashMap::new();

        // Per-module resolution
        for (source_file, module) in &graph.modules {
            // New resolver for thos module
            let resolver = BindingResolver::new(module, path_simplifier, loader);
            match resolver.resolve_bindings() {
                // If successful, then add to the overall map else add to errors
                Ok(map) => {
                    modules.insert(Rc::clone(source_file), map);
                }
                Err(module_errors) => {
                    errors.push(CrossModuleError::PerModuleErrors {
                        source_file: Rc::clone(source_file),
                        errors: module_errors,
                    });
                }
            }
        }

        // Return before bothering with cross module resolution as one of
        // the above modules failed in resolution (no point)
        if !errors.is_empty() {
            return Err(errors);
        }

        // We need to both read from some maps and write to others to resolve
        // all the cross module resolutions in the end
        //
        // In order to handle them all without annoying rust, we resolve them all out
        // here first into an overall vec and then apply the writing out after
        let mut pending_resolutions: Vec<(
            Rc<SourceFile<FileName>>,
            NodeId,                  
            ModuleFieldResolution<FileName>,
        )> = Vec::new();

        for (source_file, module_map) in &modules {
            for pending in &module_map.pending_module_accesses {
                // Recover the `Import` binding that was field-accessed
                let Some(import_binding) = module_map
                    .scope_tree
                    .lookup_binding(&pending.import_binding_id)
                else {
                    continue;
                };

                let BindingKind::Import { path: import_path } = &import_binding.kind.kind else {
                    // Should be unreachable since resolver handles, whatever if its something else then gg
                    continue;
                };

                let location = SourceLocation::new(Rc::clone(source_file), pending.span);

                // If the target module isn't in the map it failed phase 1 and
                // was already reported, so we just continue whatever
                let Some(target_map) = modules.get(import_path.as_ref()) else {
                    continue;
                };

                // Find if this is a public export
                match target_map.find_public_export(&pending.field) {

                    // A valid cross-module resolution
                    Some((binding_id, Publicity::Public)) => {
                        pending_resolutions.push((
                            Rc::clone(source_file),
                            pending.access_node_id,
                            ModuleFieldResolution {
                                target_module: Rc::clone(import_path),
                                binding_id,
                            },
                        ));
                    }

                    // Some private export
                    Some((_, Publicity::Private)) => {
                        errors.push(CrossModuleError::PrivateExport {
                            location,
                            module_path: Rc::clone(import_path),
                            field: pending.field.clone(),
                        });
                    }

                    // Doesn't exist!/no such export
                    None => {
                        errors.push(CrossModuleError::NoSuchExport {
                            location,
                            module_path: Rc::clone(import_path),
                            field: pending.field.clone(),
                        });
                    }
                }
            }
        }

        // If an error occured during resolution there is no point to apply.
        if !errors.is_empty() {
            return Err(errors);
        }

        // Apply collected resolutions now since we're done borrowing modules
        for (source_file, node_id, resolution) in pending_resolutions {
            modules
                .get_mut(&source_file)
                .expect("we literally just iterated over this source_file it must exist here")
                .module_field_resolutions
                .insert(node_id, resolution);
        }

        Ok(CrossModuleResolutionMap { modules })
    }
}
