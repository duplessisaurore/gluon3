use core::fmt::Display;

use alloc::{rc::Rc, string::String, vec::Vec};
use gluon_debug::{Located, SourceLocation, Span};
use gluon_parser::ast::{ExprKind, Module, NodeId};
use hashbrown::HashMap;

use crate::{
    binding_trait::PathSimplifier, bindings::{BindingId, BindingKind, FunctionId, ScopeTree}, errors::{BindingResolveError, BindingResolveErrorKind, BindingResolveResult},
};

/// The output of the binding resolution/name resolution phase
///
/// This provides all the mappings for the AST tree produced
/// by the `Parser`
#[derive(Debug)]
pub struct BindingResolutionMap {
    /// A mapping of AST `node_id`'s to the `BindingId` if
    /// it resolved to/defined some binding
    pub resolutions: HashMap<NodeId, BindingId>,

    /// The fully built scope tree
    pub scope_tree: ScopeTree,

    /// What bindings each function closes over, by FunctionId.
    pub captures: HashMap<FunctionId, Vec<BindingId>>,
}

/// The actual binding resolver clas itself
pub struct BindingResolver<'module, FileName: Display + Clone + PartialEq, PS: PathSimplifier> {
    /// The current output resolution map being built
    resolution_map: BindingResolutionMap,

    /// The current module being resolved
    module: &'module Module<FileName>,

    /// All errors accumulated during binding resolution
    errors: Vec<BindingResolveError<FileName, PS::PathSimplificationError>>,

    /// The last allocated function ID, we increasingly increment this
    /// monotonically to continue having unique FunctionIds's.
    next_function_id: usize,

    /// The path simplifier we are using for imports
    path_simplifier: PS
}

impl<'module, FileName: Display + Clone + PartialEq, PS: PathSimplifier> BindingResolver<'module, FileName, PS> {
    /// Create a new binding resolver over `module` that will resolve all of the
    /// bindings into a singular `BindingResolutionMap` for this module
    pub fn new(module: &'module Module<FileName>, path_simplifier: PS) -> Self {
        Self {
            resolution_map: BindingResolutionMap::new(),
            errors: Vec::new(),
            module,
            next_function_id: 0,
            path_simplifier
        }
    }

    /// Runs the `BindingResolver` on the input `Module` provided to it
    /// on creation until completion or an error.
    ///
    /// This will return in the success case a `BindingResolutionMap` that
    /// has the resolved bindings from ast node ids to their bindings.
    ///
    /// # Errors
    ///
    /// This may error in many ways!! See `BindingResolveErrorKind`, generally
    /// if a binding is not resolved, or some illegal operation is occuring.
    ///
    /// Because the binding resolver does not exit after the first error, it
    /// returns a Vec of the errors.
    pub fn resolve_bindings(
        mut self,
    ) -> Result<BindingResolutionMap, Vec<BindingResolveError<FileName, PS::PathSimplificationError>>> {
        Ok(self.resolution_map)
    }

    /// This will do a pre-pass of the `Module` to resolve top-level statements
    /// and their bindings
    ///
    /// This lets functions and stuff reference eachother regardless of declaration
    /// order
    pub fn module_pre_pass(&mut self) {
        for import in &self.module.imports {
            match import.get_kind_ref().clone() {
                ExprKind::Import { path, alias } => {
                    // If alias doesnt exist then path is the ident
                    match alias {
                        Some(alias) => {
                            // The alias will be the identifier
                            self.resolve_new_binding(alias, BindingKind::Import { path }, import.node_id)
                        },
                        None => {
                            // The simplified path will be the identifier
                            let path_ident_result = self.path_simplifier.simplify_path_to_ident(&path);

                            if let Some(path_ident) = self.recover(path_ident_result.map_err(|error| {
                                self.make_located(BindingResolveErrorKind::PathSimplificationError { path: path.clone(), error }, import.get_span())
                            })) {
                                self.resolve_new_binding(path_ident, BindingKind::Import { path }, import.node_id)
                            }
                        },
                    }
                },
                other => {
                    // Error.. just ignore and try the next import statement
                    self.errors.push(self.unexpected_expr_kind(other, import.get_span()));
                    continue;
                }
            }
        }
    }

    /// Returns an UnexpectedExprKind error for some ExprKind at some `Span`
    pub fn unexpected_expr_kind(&self, kind: ExprKind<FileName>, span: Span) -> BindingResolveError<FileName, PS::PathSimplificationError> {
        self.make_located(BindingResolveErrorKind::UnexpectedExprKind { kind: kind.clone() }, span)
    }

    /// Returns a `Located<T>` for some `T` at some `Span` in the `SourceFile`
    /// of the `Module` currently being resolved
    pub fn make_located<T>(&self, kind: T, span: Span) -> Located<T, FileName> {
        Located {
            kind,
            location: SourceLocation::new(Rc::clone(&self.module.name), span),
        }
    }

    /// Creates a new `Binding` of the kind with the `name` in the current `Scope` and then
    /// adds it as the resolution for this `node_id` to the `resolution_map`
    pub fn resolve_new_binding(&mut self, name: String, binding_kind: BindingKind, node_id: NodeId) {
        let binding_id = self.resolution_map.scope_tree.define(name, binding_kind);
        self.resolution_map.resolutions.insert(node_id, binding_id);
    }

    /// Record an error, returning Some(T) if to continue, else None
    fn recover<T>(&mut self, result: BindingResolveResult<T, FileName, PS::PathSimplificationError>) -> Option<T> {
        match result {
            Ok(v) => Some(v),
            Err(e) => {
                self.errors.push(e);
                None
            }
        }
    }
}

impl BindingResolutionMap {
    /// Returns an empty new `BindingResolutionMap`
    pub fn new() -> Self {
        Self {
            resolutions: HashMap::new(),
            scope_tree: ScopeTree::new(),
            captures: HashMap::new(),
        }
    }
}
