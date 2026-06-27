use core::fmt::Display;

use alloc::{rc::Rc, string::String, vec::Vec};
use gluon_debug::{Located, SourceLocation, Span};
use gluon_parser::ast::{ExprKind, Module, NodeId};
use hashbrown::HashMap;

use crate::{
    binding_trait::PathSimplifier,
    bindings::{Binding, BindingId, BindingKind, FunctionId, ScopeId, ScopeTree},
    errors::{BindingResolveError, BindingResolveErrorKind, BindingResolveResult},
};

/// The output of the binding resolution/name resolution phase
///
/// This provides all the mappings for the AST tree produced
/// by the `Parser`
#[derive(Debug)]
pub struct BindingResolutionMap<FileName: Display + Clone + PartialEq> {
    /// A mapping of AST `node_id`'s to the `BindingId` if
    /// it resolved to/defined some binding
    pub resolutions: HashMap<NodeId, BindingId>,

    /// The fully built scope tree
    pub scope_tree: ScopeTree<FileName>,

    /// What bindings each function closes over, by FunctionId.
    pub captures: HashMap<FunctionId, Vec<BindingId>>,
}

/// The actual binding resolver clas itself
pub struct BindingResolver<'module, FileName: Display + Clone + PartialEq, PS: PathSimplifier> {
    /// The current output resolution map being built
    resolution_map: BindingResolutionMap<FileName>,

    /// The current module being resolved
    module: &'module Module<FileName>,

    /// All errors accumulated during binding resolution
    errors: Vec<BindingResolveError<FileName, PS::PathSimplificationError>>,

    /// The last allocated function ID, we increasingly increment this
    /// monotonically to continue having unique FunctionIds's.
    next_function_id: usize,

    /// The path simplifier we are using for imports
    path_simplifier: PS,
}

impl<'module, FileName: Display + Clone + PartialEq, PS: PathSimplifier>
    BindingResolver<'module, FileName, PS>
{
    /// Create a new binding resolver over `module` that will resolve all of the
    /// bindings into a singular `BindingResolutionMap` for this module
    pub fn new(module: &'module Module<FileName>, path_simplifier: PS) -> Self {
        Self {
            resolution_map: BindingResolutionMap::new(),
            errors: Vec::new(),
            module,
            next_function_id: 0,
            path_simplifier,
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
    ) -> Result<
        BindingResolutionMap<FileName>,
        Vec<BindingResolveError<FileName, PS::PathSimplificationError>>,
    > {
        // Register all module TLS so that things can refer to eachother 
        self.module_pre_pass();


        if !self.errors.is_empty() {
            return Err(self.errors);
        }

        Ok(self.resolution_map)
    }

    /// This will do a pre-pass of the `Module` to resolve top-level statements
    /// and their bindings
    ///
    /// This lets functions and stuff reference eachother regardless of declaration
    /// order
    pub fn module_pre_pass(&mut self) {
        self.import_pre_pass();
        self.function_pre_pass();
        self.typedef_pre_pass();
        self.macro_pre_pass();
    }

    /// This will do a pre-pass of the `Module` to resolve all import statements
    pub fn import_pre_pass(&mut self) {
        for import in &self.module.imports {
            match import.get_kind_ref().clone() {
                ExprKind::Import { path, alias } => {
                    // If alias doesnt exist then path is the ident
                    match alias {
                        Some(alias) => {
                            // The alias will be the identifier

                            // Ensure uniqueness
                            if let Some(binding) = self.resolve_binding(&alias) {
                                self.errors.push(
                                    self.duplicate_top_level_defn(binding, import.get_span()),
                                );
                                continue;
                            }

                            // As it's unique, resolve it as a new binding
                            self.resolve_new_binding(
                                alias,
                                BindingKind::Import { path },
                                import.node_id,
                                import.get_span(),
                            )
                        }
                        None => {
                            // The simplified path will be the identifier
                            let path_ident_result =
                                self.path_simplifier.simplify_path_to_ident(&path);

                            if let Some(path_ident) =
                                self.recover(path_ident_result.map_err(|error| {
                                    self.make_located(
                                        BindingResolveErrorKind::PathSimplificationError {
                                            path: path.clone(),
                                            error,
                                        },
                                        import.get_span(),
                                    )
                                }))
                            {
                                // Ensure uniqueness
                                if let Some(binding) = self.resolve_binding(&path_ident) {
                                    self.errors.push(
                                        self.duplicate_top_level_defn(binding, import.get_span()),
                                    );
                                    continue;
                                }

                                self.resolve_new_binding(
                                    path_ident,
                                    BindingKind::Import { path },
                                    import.node_id,
                                    import.get_span(),
                                )
                            }
                        }
                    }
                }
                other => {
                    // Error.. just ignore and try the next import statement
                    self.errors
                        .push(self.unexpected_expr_kind("<import>", other, import.get_span()));
                    continue;
                }
            }
        }
    }


    /// This will do a pre-pass of the `Module` to resolve all function statements
    pub fn function_pre_pass(&mut self) {
        for function in &self.module.functions {
            match function.get_kind_ref().clone() {
                ExprKind::FunctionDef { name, publicity, .. } => {
                    let Some(fn_name) = name else {
                        continue;
                    };

                    // Check for uniqueness
                    if let Some(binding) = self.resolve_binding(&fn_name) {
                        self.errors.push(
                            self.duplicate_top_level_defn(binding, function.get_span()),
                        );
                        continue;
                    }

                    // Allocate a unique function ID for this function.
                    let function_id = self.allocate_function_id();
                    self.resolve_new_binding(
                        fn_name,
                        BindingKind::Function { id: function_id, publicity },
                        function.node_id,
                        function.get_span(),
                    )
                }
                other => {
                    // Error.. just ignore and try the next statement
                    self.errors
                        .push(self.unexpected_expr_kind("<function>", other, function.get_span()));
                    continue;
                }
            }
        }
    }

    /// This will do a pre-pass of the `Module` to resolve all typedef statements
    pub fn typedef_pre_pass(&mut self) {
        for typedef in &self.module.types {
            match typedef.get_kind_ref().clone() {
                ExprKind::TypeDef { name, publicity, .. } => {
                    // Check for uniqueness
                    if let Some(binding) = self.resolve_binding(&name) {
                        self.errors.push(
                            self.duplicate_top_level_defn(binding, typedef.get_span()),
                        );
                        continue;
                    }

                    self.resolve_new_binding(
                        name,
                        BindingKind::Type { publicity },
                        typedef.node_id,
                        typedef.get_span(),
                    )
                }
                other => {
                    // Error.. just ignore and try the next statement
                    self.errors
                        .push(self.unexpected_expr_kind("<type>", other, typedef.get_span()));
                    continue;
                }
            }
        }
    }

    /// This will do a pre-pass of the `Module` to resolve all macro statements
    pub fn macro_pre_pass(&mut self) {
        for macrodef in &self.module.macros {
            match macrodef.get_kind_ref().clone() {
                ExprKind::MacroDef { name, publicity, .. } => {
                    // Check for uniqueness
                    if let Some(binding) = self.resolve_binding(&name) {
                        self.errors.push(
                            self.duplicate_top_level_defn(binding, macrodef.get_span()),
                        );
                        continue;
                    }

                    self.resolve_new_binding(
                        name,
                        BindingKind::Macro { publicity },
                        macrodef.node_id,
                        macrodef.get_span(),
                    )
                }
                other => {
                    // Error.. just ignore and try the next statement
                    self.errors
                        .push(self.unexpected_expr_kind("<macro>", other, macrodef.get_span()));
                    continue;
                }
            }
        }
    }

    /// Returns an UnexpectedExprKind error for some ExprKind at some `Span`
    pub fn unexpected_expr_kind(
        &self,
        expected: impl Into<String>,
        kind: ExprKind<FileName>,
        span: Span,
    ) -> BindingResolveError<FileName, PS::PathSimplificationError> {
        self.make_located(
            BindingResolveErrorKind::UnexpectedExprKind { expected: expected.into(), kind: kind.clone() },
            span,
        )
    }

    /// Returns an DuplicateTopLevelDefinition error for some binding
    ///
    /// The span passed in is where the duplicate occured, as opposed
    /// to the binding being the original
    pub fn duplicate_top_level_defn(
        &self,
        binding: &Binding<FileName>,
        span: Span,
    ) -> BindingResolveError<FileName, PS::PathSimplificationError> {
        self.make_located(
            BindingResolveErrorKind::DuplicateTopLevelDefinition {
                name: binding.kind.name.clone(),
                original: binding.location.clone(),
            },
            span,
        )
    }

    /// Resolves the passed in `name` starting from the current `Scope` up to the
    /// `Module` scope using the `ScopeTree`
    pub fn resolve_name(&self, name: &str) -> Option<(BindingId, ScopeId)> {
        self.resolution_map.scope_tree.resolve_name(name)
    }

    /// Tries find a binding from the current scope up to the top level
    /// `Module` scope.
    pub fn resolve_binding(&self, name: &str) -> Option<&Binding<FileName>> {
        // Find the binding id
        if let Some((binding_id, _)) = self.resolve_name(name) {
            // Lookup binding to make sure it actually exists
            if let Some(binding) = self.resolution_map.scope_tree.lookup_binding(&binding_id) {
                return Some(binding);
            }
        };

        None
    }

    /// Returns a `Located<T>` for some `T` at some `Span` in the `SourceFile`
    /// of the `Module` currently being resolved
    pub fn make_located<T>(&self, kind: T, span: Span) -> Located<T, FileName> {
        Located {
            kind,
            location: self.make_location(span),
        }
    }

    /// Returns a `SourceLocation` into the `Module`'s file with the provided
    /// `Span`
    pub fn make_location(&self, span: Span) -> SourceLocation<FileName> {
        SourceLocation::new(Rc::clone(&self.module.name), span)
    }

    /// Creates a new `Binding` of the kind with the `name` in the current `Scope` and then
    /// adds it as the resolution for this `node_id` to the `resolution_map`
    ///
    /// The `Span` is the location of which this new binding was resolved
    pub fn resolve_new_binding(
        &mut self,
        name: String,
        binding_kind: BindingKind,
        node_id: NodeId,
        span: Span,
    ) {
        let binding_id =
            self.resolution_map
                .scope_tree
                .define(name, binding_kind, self.make_location(span));
        self.resolution_map.resolutions.insert(node_id, binding_id);
    }

    /// Record an error, returning Some(T) if to continue, else None
    fn recover<T>(
        &mut self,
        result: BindingResolveResult<T, FileName, PS::PathSimplificationError>,
    ) -> Option<T> {
        match result {
            Ok(v) => Some(v),
            Err(e) => {
                self.errors.push(e);
                None
            }
        }
    }

    /// Allocates a new unique `FunctionId`
    fn allocate_function_id(&mut self) -> FunctionId {
        self.next_function_id += 1;
        FunctionId(self.next_function_id)
    }
}

impl<FileName: Display + PartialEq + Clone> BindingResolutionMap<FileName> {
    /// Returns an empty new `BindingResolutionMap`
    pub fn new() -> Self {
        Self {
            resolutions: HashMap::new(),
            scope_tree: ScopeTree::new(),
            captures: HashMap::new(),
        }
    }
}
