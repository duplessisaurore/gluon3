//! The actual binding resolver itself, this
//! resolves all bindings in a module and makes sure
//! that everything is defined as well as it can.

use core::fmt::Display;

use alloc::{
    rc::Rc,
    string::{String, ToString},
    vec::Vec,
};
use gluon_debug::{Located, SourceFile, SourceLocation, Span};
use gluon_module_resolver::LoadModule;
use gluon_parser::ast::{
    ArrayElement, AstNode, ExprKind, Module, NodeId, ObjectElement, Pattern, PatternNode,
    PatternObjectLikeFields, Publicity,
};
use hashbrown::HashMap;

use crate::{
    Builtins,
    binding_trait::PathSimplifier,
    bindings::{Binding, BindingId, BindingKind, FunctionId, ScopeBoundary, ScopeId, ScopeTree},
    errors::{BindingResolveError, BindingResolveErrorKind, BindingResolveResult},
};

/// A cross-field resolution to a different module
/// with a different `ScopeTree`
#[derive(Debug)]
pub struct ModuleFieldResolution<FileName: Display + Clone + PartialEq> {
    /// Which module's ScopeTree the binding_id lives in
    pub target_module: Rc<SourceFile<FileName>>,
    pub binding_id: BindingId,
}

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

    /// Field accesses on module imports that are pending from this
    /// one module.
    ///
    /// This is pub(crate) because it is only visible to this phase while
    /// we are working on building `module_field_resolutions`
    pub(crate) pending_module_accesses: Vec<PendingModuleAccess>,

    /// Field accesses to a module that have been resolved and
    /// point to a binding in a seperate module
    pub module_field_resolutions: HashMap<NodeId, ModuleFieldResolution<FileName>>,
}

/// This is a module field access that is pending
///
/// The current resolver here only resolves per-module
/// as opposed to cross-module, so we record it's pending
/// cross-module accesses for the later cross module phase
/// to use during it's resolving.
#[derive(Debug)]
pub struct PendingModuleAccess {
    /// NodeId of the FieldAccess node itself.
    pub access_node_id: NodeId,

    /// The BindingId of the Import binding being field-accessed.
    pub import_binding_id: BindingId,

    /// The field name in the module being accessed.
    pub field: String,

    /// Span of the field access.
    pub span: Span,
}

/// The actual binding resolver clas itself
pub struct BindingResolver<
    'module,
    'loader,
    'simplifier,
    'builtins,
    FileName: Display + Clone + PartialEq,
    PS: PathSimplifier,
    Loader: LoadModule<FileName>,
> {
    /// The current output resolution map being built
    resolution_map: BindingResolutionMap<FileName>,

    /// The current module being resolved
    module: &'module Module<FileName>,

    /// All errors accumulated during binding resolution
    errors:
        Vec<BindingResolveError<FileName, PS::PathSimplificationError, Loader::ResolveSourceError>>,

    /// The last allocated function ID, we increasingly increment this
    /// monotonically to continue having unique FunctionIds's.
    next_function_id: usize,

    /// The path simplifier we are using for imports
    path_simplifier: &'simplifier mut PS,

    /// The loader. This is important for imports as we need
    /// to resolve it down to a source file again since the module
    /// loader only builds the ResolvedGraph.
    loader: &'loader mut Loader,

    /// The set of builtins we are using for resolving
    builtins: &'builtins Builtins,
}

impl<
    'module,
    'loader,
    'simplifier,
    'builtins,
    FileName: Display + Clone + PartialEq,
    PS: PathSimplifier,
    Loader: LoadModule<FileName>,
> BindingResolver<'module, 'loader, 'simplifier, 'builtins, FileName, PS, Loader>
{
    /// Create a new binding resolver over `module` that will resolve all of the
    /// bindings into a singular `BindingResolutionMap` for this module
    pub fn new(
        module: &'module Module<FileName>,
        path_simplifier: &'simplifier mut PS,
        loader: &'loader mut Loader,
        builtins: &'builtins Builtins,
    ) -> Self {
        Self {
            resolution_map: BindingResolutionMap::new(),
            errors: Vec::new(),
            module,
            next_function_id: 0,
            path_simplifier,
            loader,
            builtins,
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
        Vec<BindingResolveError<FileName, PS::PathSimplificationError, Loader::ResolveSourceError>>,
    > {
        // Register all the builtins so they don't fail later during name resolution
        self.register_builtins();

        // Register all module TLS so that things can refer to eachother
        self.module_pre_pass();

        // Resolve bodies of all top-level items now
        for import in &self.module.imports.clone() {
            self.resolve_expr(import);
        }
        for typedef in &self.module.types.clone() {
            self.resolve_expr(typedef);
        }
        for function in &self.module.functions.clone() {
            self.resolve_expr(function);
        }
        for macrodef in &self.module.macros.clone() {
            self.resolve_expr(macrodef);
        }

        // Handle module level `Let`'s here to tell between `Local`'s and `Let`'s.
        // and then also every other statement.
        for stmt in &self.module.statements {
            match stmt.get_kind_ref() {
                // The actual inner pattern binding stuff was alr registered above so not much here other
                // than resolving_expr
                ExprKind::LetBinding {
                    annotation,
                    initializer,
                    ..
                } => {
                    if let Some(ann) = annotation {
                        self.resolve_expr(ann);
                    }
                    self.resolve_expr(initializer);
                }
                _ => {
                    self.resolve_expr(stmt);
                }
            }
        }

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
        self.let_pre_pass()
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

                            // Resolve path again down to the SourceFile
                            let resolved_path_result =
                                self.loader.resolve_source_file(&path).map_err(|error| {
                                    self.make_located(
                                        BindingResolveErrorKind::PathResolveError { error },
                                        import.get_span(),
                                    )
                                });
                            let Some(resolved_path) = self.recover(resolved_path_result) else {
                                continue;
                            };

                            // As it's unique, resolve it as a new binding
                            self.resolve_new_binding(
                                alias,
                                BindingKind::Import {
                                    path: Rc::new(resolved_path),
                                },
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

                                // Resolve path again down to the SourceFile
                                let resolved_path_result =
                                    self.loader.resolve_source_file(&path).map_err(|error| {
                                        self.make_located(
                                            BindingResolveErrorKind::PathResolveError { error },
                                            import.get_span(),
                                        )
                                    });
                                let Some(resolved_path) = self.recover(resolved_path_result) else {
                                    continue;
                                };

                                self.resolve_new_binding(
                                    path_ident,
                                    BindingKind::Import {
                                        path: Rc::new(resolved_path),
                                    },
                                    import.node_id,
                                    import.get_span(),
                                )
                            }
                        }
                    }
                }
                other => {
                    // Error.. just ignore and try the next import statement
                    self.errors.push(self.unexpected_expr_kind(
                        "<import>",
                        other,
                        import.get_span(),
                    ));
                    continue;
                }
            }
        }
    }

    /// This will do a pre-pass of the `Module` to resolve all function statements
    pub fn function_pre_pass(&mut self) {
        for function in &self.module.functions {
            match function.get_kind_ref() {
                ExprKind::FunctionDef {
                    name, publicity, ..
                } => {
                    let Some(fn_name) = name else {
                        continue;
                    };

                    // Check for uniqueness
                    if let Some(binding) = self.resolve_binding(&fn_name) {
                        self.errors
                            .push(self.duplicate_top_level_defn(binding, function.get_span()));
                        continue;
                    }

                    // Allocate a unique function ID for this function.
                    let function_id = self.allocate_function_id();
                    self.resolve_new_binding(
                        fn_name.clone(),
                        BindingKind::Function {
                            id: function_id,
                            publicity: *publicity,
                        },
                        function.node_id,
                        function.get_span(),
                    )
                }
                other => {
                    // Error.. just ignore and try the next statement
                    self.errors.push(self.unexpected_expr_kind(
                        "<function>",
                        other.clone(),
                        function.get_span(),
                    ));
                    continue;
                }
            }
        }
    }

    /// This will do a pre-pass of the `Module` to resolve all typedef statements
    pub fn typedef_pre_pass(&mut self) {
        for typedef in &self.module.types {
            match typedef.get_kind_ref() {
                ExprKind::TypeDef {
                    name, publicity, ..
                } => {
                    // Check for uniqueness
                    if let Some(binding) = self.resolve_binding(&name) {
                        self.errors
                            .push(self.duplicate_top_level_defn(binding, typedef.get_span()));
                        continue;
                    }

                    self.resolve_new_binding(
                        name.clone(),
                        BindingKind::Type {
                            publicity: *publicity,
                        },
                        typedef.node_id,
                        typedef.get_span(),
                    )
                }
                other => {
                    // Error.. just ignore and try the next statement
                    self.errors.push(self.unexpected_expr_kind(
                        "<type>",
                        other.clone(),
                        typedef.get_span(),
                    ));
                    continue;
                }
            }
        }
    }

    /// This will do a pre-pass of the `Module` to resolve all macro statements
    pub fn macro_pre_pass(&mut self) {
        for macrodef in &self.module.macros {
            match macrodef.get_kind_ref() {
                ExprKind::MacroDef {
                    name, publicity, ..
                } => {
                    // Check for uniqueness
                    if let Some(binding) = self.resolve_binding(&name) {
                        self.errors
                            .push(self.duplicate_top_level_defn(binding, macrodef.get_span()));
                        continue;
                    }

                    self.resolve_new_binding(
                        name.clone(),
                        BindingKind::Macro {
                            publicity: *publicity,
                        },
                        macrodef.node_id,
                        macrodef.get_span(),
                    )
                }
                other => {
                    // Error.. just ignore and try the next statement
                    self.errors.push(self.unexpected_expr_kind(
                        "<macro>",
                        other.clone(),
                        macrodef.get_span(),
                    ));
                    continue;
                }
            }
        }
    }

    /// This will do a pre-pass of the `Module` to resolve all Global lets
    pub fn let_pre_pass(&mut self) {
        for letdef in &self.module.statements {
            match letdef.get_kind_ref() {
                ExprKind::LetBinding {
                    is_mutable,
                    pattern,
                    publicity,
                    ..
                } => {
                    // Register the binding with the patterns in mind.
                    self.introduce_pattern_bindings(
                        pattern,
                        BindingKind::Let {
                            is_mutable: *is_mutable,
                            publicity: *publicity,
                        },
                    );
                }
                _ => {}
            }
        }
    }

    /// Introduce the bindings given by a pattern into the current scope
    fn introduce_pattern_bindings(
        &mut self,
        pattern: &PatternNode<FileName>,
        kind: BindingKind<FileName>,
    ) {
        match pattern.get_kind_ref() {
            // Neither introduces any binding
            Pattern::Wildcard | Pattern::Lit(_) => {}

            // This introduces the identifier
            Pattern::Identifier(name) => {
                self.resolve_new_binding(name.clone(), kind, pattern.node_id, pattern.get_span());
            }

            // This introduces all of the before, rest and after patterns.
            Pattern::Array {
                before,
                rest,
                after,
            } => {
                for pat in before {
                    self.introduce_pattern_bindings(pat, kind.clone());
                }
                if let Some(rest_pat) = rest {
                    self.introduce_pattern_bindings(rest_pat, kind.clone());
                }
                for pat in after {
                    self.introduce_pattern_bindings(pat, kind.clone());
                }
            }

            // This introduces all the fields as bindings
            Pattern::Object {
                target_type,
                fields,
            } => {
                // Resolve the type reference
                if let Some(typeref) = target_type {
                    self.resolve_expr(typeref);
                }
                self.introduce_object_like_field_bindings(fields, kind);
            }

            // Similar to the object
            Pattern::EnumVariant {
                enum_type, fields, ..
            } => {
                self.resolve_expr(enum_type);
                if let Some(fields) = fields {
                    self.introduce_object_like_field_bindings(fields, kind);
                }
            }

            // Macros, todo later
            Pattern::Quote(body) => {
                todo!()
            }
            Pattern::UnhygienicIdentifier(name) => {
                todo!()
            }
        }
    }

    /// Introduces bindings for object and enum variant field
    /// patterns since they have identical field types
    fn introduce_object_like_field_bindings(
        &mut self,
        fields: &PatternObjectLikeFields<FileName>,
        kind: BindingKind<FileName>,
    ) {
        for field in fields {
            self.introduce_pattern_bindings(&field.payload, kind.clone());
        }
    }

    /// Returns an UnexpectedExprKind error for some ExprKind at some `Span`
    pub fn unexpected_expr_kind(
        &self,
        expected: impl Into<String>,
        kind: ExprKind<FileName>,
        span: Span,
    ) -> BindingResolveError<FileName, PS::PathSimplificationError, Loader::ResolveSourceError>
    {
        self.make_located(
            BindingResolveErrorKind::UnexpectedExprKind {
                expected: expected.into(),
                kind: kind.clone(),
            },
            span,
        )
    }

    /// Returns an DuplicateTopLevelDefinition/RedefineBuiltin error for some binding
    ///
    /// The span passed in is where the duplicate occured, as opposed
    /// to the binding being the original
    pub fn duplicate_top_level_defn(
        &self,
        binding: &Binding<FileName>,
        span: Span,
    ) -> BindingResolveError<FileName, PS::PathSimplificationError, Loader::ResolveSourceError>
    {
        self.make_located(
            match binding.kind.kind {
                BindingKind::Builtin => BindingResolveErrorKind::RedefineBuiltin {
                    name: binding.kind.name.clone(),
                },
                _ => BindingResolveErrorKind::DuplicateTopLevelDefinition {
                    name: binding.kind.name.clone(),
                    original: binding.location.clone(),
                },
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
        name: impl Into<String>,
        binding_kind: BindingKind<FileName>,
        node_id: NodeId,
        span: Span,
    ) {
        let binding_id = self.resolution_map.scope_tree.define(
            name.into(),
            binding_kind,
            self.make_location(span),
        );
        self.resolution_map.resolutions.insert(node_id, binding_id);
    }

    /// Record an error, returning Some(T) if to continue, else None
    fn recover<T>(
        &mut self,
        result: BindingResolveResult<
            T,
            FileName,
            PS::PathSimplificationError,
            Loader::ResolveSourceError,
        >,
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

    /// Resolves all name bindings within an expression node, recording
    /// resolutions in the map, introducing new bindings, and detecting captures.
    fn resolve_expr(&mut self, node: &AstNode<FileName>) {
        match node.get_kind_ref() {
            // Nothing to resolve for these
            // as they're simple or handled by another phase.
            ExprKind::Lit(_)
            | ExprKind::Placeholder
            | ExprKind::Continue
            | ExprKind::Import { .. } => {}

            // An identifier here which refers to some existing binding
            ExprKind::Identifier(name) => match self.resolve_name(name) {
                Some((binding_id, found_scope_id)) => {
                    self.resolution_map
                        .resolutions
                        .insert(node.node_id, binding_id);
                    self.check_capture(binding_id, found_scope_id);
                }
                None => {
                    self.errors.push(self.make_located(
                        BindingResolveErrorKind::UnresolvedName { name: name.clone() },
                        node.get_span(),
                    ));
                }
            },

            // Macros.. todo
            ExprKind::UnhygienicIdentifier(name) => {
                todo!()
            }

            // Resolve all the parts of the StrInterp
            ExprKind::StrInterp(parts) => {
                for part in parts {
                    self.resolve_expr(part);
                }
            }

            // Resolve each element in the array literal (it must refer to something)
            ExprKind::ArrayLiteral(elements) => {
                for element in elements {
                    match element {
                        ArrayElement::Normal(expr) | ArrayElement::Spread(expr) => {
                            self.resolve_expr(expr);
                        }
                    }
                }
            }

            // Resolve each element and type in the object literal as they're both expressions.
            ExprKind::ObjectLiteral {
                target_type,
                elements,
            } => {
                if let Some(typeref) = target_type {
                    self.resolve_expr(typeref);
                }
                for element in elements {
                    match element {
                        ObjectElement::Field(field) => self.resolve_expr(&field.payload),
                        ObjectElement::Spread(expr) => self.resolve_expr(expr),
                    }
                }
            }

            // Same as object
            ExprKind::EnumVariantLiteral {
                enum_type,
                elements,
                ..
            } => {
                self.resolve_expr(enum_type);
                if let Some(elems) = elements {
                    for element in elems {
                        match element {
                            ObjectElement::Field(field) => self.resolve_expr(&field.payload),
                            ObjectElement::Spread(expr) => self.resolve_expr(expr),
                        }
                    }
                }
            }

            // Each statement inside a block needs to be resolved.
            ExprKind::Block(stmts) => {
                // A block starts a new scope.
                self.resolution_map
                    .scope_tree
                    .enter_scope(ScopeBoundary::Block);
                for stmt in stmts {
                    self.resolve_expr(stmt);
                }
                self.resolution_map.scope_tree.exit_scope();
            }

            // Module-level Let's are handled specially in resolve_bindings
            // and never reach `resolve_expr`.
            //
            // Any `LetBinding` we see here is local to a function or block,
            // so it always produces a `Local` binding.
            ExprKind::LetBinding {
                is_mutable,
                pattern,
                annotation,
                initializer,
                ..
            } => {
                if let Some(annotation_expr) = annotation {
                    self.resolve_expr(annotation_expr);
                }

                // Initializer is resolved before the pattern bindings are introduced
                // so that `let x = x + 1` resolves the RHS `x` to the prior binding
                // else we wont get proper UndeclaredNames/bindings.
                self.resolve_expr(initializer);
                self.introduce_pattern_bindings(
                    pattern,
                    BindingKind::Local {
                        is_mutable: *is_mutable,
                    },
                );
            }

            // A function definition
            ExprKind::FunctionDef {
                name,
                type_params,
                params,
                return_type,
                body,
                ..
            } => {
                // Module-level named functions have their `node_id` in resolutions from
                // `function_pre_pass`. Local named functions and closures do not.
                let already_registered =
                    self.resolution_map.resolutions.contains_key(&node.node_id);

                // Retrieve the pre-registered FunctionId if it exists, otherwise allocate a
                // new one for this non-module level function
                let maybe_function_id = if already_registered {
                    self.resolution_map
                        .resolutions
                        .get(&node.node_id)
                        .and_then(|bid| self.resolution_map.scope_tree.lookup_binding(bid))
                        .and_then(|b| match &b.kind.kind {
                            BindingKind::Function { id, .. } => Some(*id),
                            _ => None,
                        })
                } else {
                    None
                };

                let function_id = maybe_function_id.unwrap_or_else(|| self.allocate_function_id());

                // Register the function if it has a name (the local one)
                // in the current scope as a local.
                //
                // This is so that in the current scope we can refer to it,
                // not only in the function's scope
                if !already_registered {
                    if let Some(fn_name) = name {
                        self.resolve_new_binding(
                            fn_name.clone(),
                            BindingKind::Local { is_mutable: false },
                            node.node_id,
                            node.get_span(),
                        );
                    }
                }

                self.resolution_map
                    .scope_tree
                    .enter_scope(ScopeBoundary::Function(function_id));

                // Type params
                //
                // Constraints are resolved before the param name is introduced,
                // since a constraint can reference earlier type params. (same thing
                // wtih the local let blah.. above )
                for type_param in type_params {
                    if let Some(constraint) = &type_param.payload {
                        self.resolve_expr(constraint);
                    }
                    self.resolution_map.scope_tree.define(
                        type_param.name.clone(),
                        BindingKind::Parameter,
                        self.make_location(node.get_span()),
                    );
                }

                // Value params
                for param in params {
                    if let Some(ann) = &param.annotation {
                        self.resolve_expr(ann);
                    }
                    self.introduce_pattern_bindings(&param.name, BindingKind::Parameter);
                }

                // Resolve the return type as a proper binding to make
                // sure it's properly defined.
                if let Some(ret) = return_type {
                    self.resolve_expr(ret);
                }

                // Resolve the body in the scope
                self.resolve_expr(body);
                self.resolution_map.scope_tree.exit_scope();
            }

            // Macro todo
            ExprKind::MacroDef {
                name,
                params,
                body,
                publicity,
            } => {
                todo!()
            }

            // A type definition
            //
            // We open a Block scope (not Function) so that type params are visible in
            // all elements of the type
            ExprKind::TypeDef {
                name,
                params,
                underlying_type,
                publicity,
            } => {
                // Check if we already registered this (module-level) or not yet, then register.
                let already_registered =
                    self.resolution_map.resolutions.contains_key(&node.node_id);
                if !already_registered {
                    self.resolve_new_binding(
                        name.clone(),
                        BindingKind::Type {
                            publicity: *publicity,
                        },
                        node.node_id,
                        node.get_span(),
                    );
                }

                // Enter scope so we get all the type params to this typedef like a function kinda.
                self.resolution_map
                    .scope_tree
                    .enter_scope(ScopeBoundary::Block);

                // Resolve each type param in the same way as a function..
                for type_param in params {
                    if let Some(constraint) = &type_param.payload {
                        self.resolve_expr(constraint);
                    }
                    self.resolution_map.scope_tree.define(
                        type_param.name.clone(),
                        BindingKind::Parameter,
                        self.make_location(node.get_span()),
                    );
                }

                // Resolve the underlying type...
                self.resolve_expr(underlying_type);

                // Done!
                self.resolution_map.scope_tree.exit_scope();
            }

            ExprKind::TypeGuard { base, guard } => {
                self.resolve_expr(base);
                self.resolve_expr(guard);
            }

            ExprKind::TypeFail { base, fail_message } => {
                self.resolve_expr(base);
                self.resolve_expr(fail_message);
            }

            ExprKind::TypeWith { base, methods } => {
                self.resolve_expr(base);
                for method in methods {
                    self.resolve_expr(method);
                }
            }

            ExprKind::ObjectTypeDef { fields } => {
                for field in fields {
                    self.resolve_expr(&field.payload);
                }
            }

            // Similar to an object but kind-of like nested? blehh
            ExprKind::EnumTypeDef { variants } => {
                for variant in variants {
                    if let Some(fields) = &variant.fields {
                        for field in fields {
                            self.resolve_expr(&field.payload);
                        }
                    }
                }
            }

            ExprKind::FunctionType {
                params,
                return_type,
            } => {
                for param in params {
                    self.resolve_expr(param);
                }
                self.resolve_expr(return_type);
            }

            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                // First the condition
                self.resolve_expr(condition);

                // The branches are all scoped
                self.resolution_map
                    .scope_tree
                    .enter_scope(ScopeBoundary::Block);
                self.resolve_expr(then_branch);
                self.resolution_map.scope_tree.exit_scope();

                if let Some(else_b) = else_branch {
                    self.resolution_map
                        .scope_tree
                        .enter_scope(ScopeBoundary::Block);
                    self.resolve_expr(else_b);
                    self.resolution_map.scope_tree.exit_scope();
                }
            }

            ExprKind::Match { subject, arms } => {
                self.resolve_expr(subject);
                for arm in arms {
                    // Each arm gets its own scope.
                    self.resolution_map
                        .scope_tree
                        .enter_scope(ScopeBoundary::Block);

                    // The actual match pattern binding which maybe introduced
                    self.introduce_pattern_bindings(
                        &arm.pattern,
                        BindingKind::Local { is_mutable: false },
                    );

                    // Optional guard..
                    if let Some(guard) = &arm.guard {
                        self.resolve_expr(guard);
                    }

                    self.resolve_expr(&arm.body);
                    self.resolution_map.scope_tree.exit_scope();
                }
            }

            ExprKind::While { condition, body } => {
                self.resolve_expr(condition);

                // The body of the each loop is its own scope..
                self.resolution_map
                    .scope_tree
                    .enter_scope(ScopeBoundary::Block);
                self.resolve_expr(body);
                self.resolution_map.scope_tree.exit_scope();
            }

            ExprKind::For {
                pattern,
                iterable,
                body,
            } => {
                self.resolve_expr(iterable);

                self.resolution_map
                    .scope_tree
                    .enter_scope(ScopeBoundary::Block);
                self.introduce_pattern_bindings(pattern, BindingKind::Local { is_mutable: false });
                self.resolve_expr(body);
                self.resolution_map.scope_tree.exit_scope();
            }

            ExprKind::Loop { body } => {
                self.resolution_map
                    .scope_tree
                    .enter_scope(ScopeBoundary::Block);
                self.resolve_expr(body);
                self.resolution_map.scope_tree.exit_scope();
            }

            ExprKind::TryCatch {
                try_body,
                error_binding,
                catch_body,
            } => {
                // The try and catch bodies are in separate scopes.
                //
                // As the error binding should only be
                // visible inside the catch body, not the try body
                self.resolution_map
                    .scope_tree
                    .enter_scope(ScopeBoundary::Block);
                self.resolve_expr(try_body);
                self.resolution_map.scope_tree.exit_scope();

                self.resolution_map
                    .scope_tree
                    .enter_scope(ScopeBoundary::Block);
                self.introduce_pattern_bindings(
                    error_binding,
                    BindingKind::Local { is_mutable: false },
                );
                self.resolve_expr(catch_body);
                self.resolution_map.scope_tree.exit_scope();
            }

            ExprKind::Defer(expr) => {
                self.resolve_expr(expr);
            }

            ExprKind::TypeCast { expr, target_type } => {
                self.resolve_expr(expr);
                self.resolve_expr(target_type);
            }

            ExprKind::TypeCheck { expr, target_type } => {
                self.resolve_expr(expr);
                self.resolve_expr(target_type);
            }

            ExprKind::Assignment { target, value } => {
                self.resolve_expr(target);
                self.resolve_expr(value);
                self.check_assignment_target(target);
            }

            ExprKind::CompoundAssignment { op, target, value } => {
                self.resolve_expr(op);
                self.resolve_expr(target);
                self.resolve_expr(value);
                self.check_assignment_target(target);
            }

            // op is a function used as an infix operator — it must resolve to a binding
            ExprKind::BinaryOp { op, left, right } => {
                self.resolve_expr(op);
                self.resolve_expr(left);
                self.resolve_expr(right);
            }

            ExprKind::UnaryOp { op, expr } => {
                self.resolve_expr(op);
                self.resolve_expr(expr);
            }

            ExprKind::Pipeline { left, right } => {
                self.resolve_expr(left);
                self.resolve_expr(right);
            }

            ExprKind::Call { callee, arguments } => {
                self.resolve_expr(callee);
                for arg in arguments {
                    self.resolve_expr(arg);
                }
            }

            // Only the expression is resolved itself.
            // We cannot really resolve the actual access itself because type information
            // is required at this point (other than for modules).
            ExprKind::FieldAccess { expr, field } => {
                self.resolve_expr(expr);

                // If the object resolved to an Import binding, then add
                // a pending module access for validation during cross module
                // resolution.
                if let Some(&binding_id) = self.resolution_map.resolutions.get(&expr.node_id) {
                    if let Some(binding) =
                        self.resolution_map.scope_tree.lookup_binding(&binding_id)
                    {
                        // We have an import! add all of the pending stuff so we can resolve it later..
                        if matches!(&binding.kind.kind, BindingKind::Import { .. }) {
                            self.resolution_map
                                .pending_module_accesses
                                .push(PendingModuleAccess {
                                    access_node_id: node.node_id,
                                    import_binding_id: binding_id,
                                    field: field.clone(),
                                    span: node.get_span(),
                                });
                        }
                    }
                }
            }

            ExprKind::IndexAccess { expr, index } => {
                self.resolve_expr(expr);
                self.resolve_expr(index);
            }

            ExprKind::Slice { array, start, end } => {
                self.resolve_expr(array);
                if let Some(s) = start {
                    self.resolve_expr(s);
                }
                if let Some(e) = end {
                    self.resolve_expr(e);
                }
            }

            ExprKind::Parametric { target, arguments } => {
                self.resolve_expr(target);
                for arg in arguments {
                    self.resolve_expr(arg);
                }
            }

            ExprKind::Break(value) => {
                if let Some(val) = value {
                    self.resolve_expr(val);
                }
            }

            ExprKind::Return(value) => {
                if let Some(val) = value {
                    self.resolve_expr(val);
                }
            }

            ExprKind::Raise(value) => {
                self.resolve_expr(value);
            }

            ExprKind::MacroInvoke {
                macro_target,
                arguments,
            } => {
                todo!()
            }

            ExprKind::MacroQuote(stmts) => {
                todo!()
            }

            ExprKind::MacroSplice(expr) => {
                todo!()
            }
        }
    }

    /// Checks whether a binding resolved from a given scope is being captured
    /// across a function scope boundary, and if so records it in the captures map.
    fn check_capture(&mut self, binding_id: BindingId, found_scope_id: ScopeId) {
        // Check the owning function of the scope with this id
        let binding_owner = self
            .resolution_map
            .scope_tree
            .owning_function(found_scope_id);

        // Check the current function
        let current_function = self.resolution_map.scope_tree.nearest_function();

        // If its the same function then no capture needed
        if binding_owner == current_function {
            return;
        }

        // Only Local and Parameter bindings need to be captured into closures.
        if !(self
            .resolution_map
            .scope_tree
            .lookup_binding(&binding_id)
            .map(|b| {
                matches!(
                    &b.kind.kind,
                    BindingKind::Local { .. } | BindingKind::Parameter
                )
            })
            .unwrap_or(false))
        {
            return;
        }

        // Get the function id of the current function capturing something outside
        // of it (in another function.)
        let Some(fn_id) = current_function else {
            return;
        };

        // Add to the captures map. (unless its already captured)
        let captures = self
            .resolution_map
            .captures
            .entry(fn_id)
            .or_insert_with(Vec::new);

        if !captures.contains(&binding_id) {
            captures.push(binding_id);
        }
    }

    /// Validates that an assignment target is mutable and assignable.
    ///
    /// We can only statically validate simple `Identifier` targets here.
    fn check_assignment_target(&mut self, target: &AstNode<FileName>) {
        let ExprKind::Identifier(_) = target.get_kind_ref() else {
            return;
        };

        // Find the actual binding to check for immutability
        let Some(&binding_id) = self.resolution_map.resolutions.get(&target.node_id) else {
            return;
        };

        let binding_info = self.resolution_map.scope_tree.lookup_binding(&binding_id);

        let Some(binding) = binding_info else {
            return;
        };

        match &binding.kind.kind {
            // Trying to assign to a immutable binding.
            BindingKind::Local { is_mutable: false }
            | BindingKind::Let {
                is_mutable: false, ..
            } => {
                self.errors.push(self.make_located(
                    BindingResolveErrorKind::AssignmentToImmutable {
                        name: binding.kind.name.clone(),
                        original: binding.location.clone(),
                    },
                    target.get_span(),
                ));
            }

            // Trying to assign to something that isnt
            // even a local...
            BindingKind::Parameter
            | BindingKind::Type { .. }
            | BindingKind::Function { .. }
            | BindingKind::Macro { .. }

            // Trying to assign to an entirely non-local thing.
            | BindingKind::Import { .. } => {
                self.errors.push(self.make_located(
                    BindingResolveErrorKind::AssignmentToNonLocal {
                        name: binding.kind.name.clone(),
                        original: binding.location.clone(),
                    },
                    target.get_span(),
                ));
            }
            _ => {}
        }
    }

    /// Registers all the builtins into the scope tree and resolution map
    /// such that they're all known
    fn register_builtins(&mut self) {
        // A synthetic location, they don't really exist.
        let synthetic_location = self.make_location(Span { start: 0, end: 0 });
        for def in self.builtins.0 {
            self.resolution_map.scope_tree.define(
                def.name.to_string(),
                BindingKind::Builtin,
                synthetic_location.clone(),
            );
        }
    }
}

impl<FileName: Display + PartialEq + Clone> BindingResolutionMap<FileName> {
    /// Returns an empty new `BindingResolutionMap`
    pub fn new() -> Self {
        Self {
            resolutions: HashMap::new(),
            scope_tree: ScopeTree::new(),
            captures: HashMap::new(),
            pending_module_accesses: Vec::new(),
            module_field_resolutions: HashMap::new(),
        }
    }

    /// Look up `name` in this module's root scope and return its Publicity
    pub fn find_public_export(&self, name: &str) -> Option<(BindingId, Publicity)> {
        // Fnd the binding in the root scope
        let (binding_id, _) = self.scope_tree.resolve_name_in_root(name)?;
        let binding = self.scope_tree.lookup_binding(&binding_id)?;

        // Return its publicity (or default to Private)
        let is_public = match &binding.kind.kind {
            BindingKind::Function { publicity, .. }
            | BindingKind::Type { publicity }
            | BindingKind::Macro { publicity }
            | BindingKind::Let { publicity, .. } => *publicity, // assuming bool; adjust if Publicity enum
            // Imports, locals, and parameters are never directly exported
            _ => Publicity::Private,
        };
        Some((binding_id, is_public))
    }
}
