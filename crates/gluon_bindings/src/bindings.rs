//! A binding represents some mapping of a name with some ID
//! of some Kind which has been bound in some `Scope`. 
//! 
//! We use these bindings to resolve names throughout
//! the AST produced by the `Parser` and to realise
//! what values are captured by a closure etc.

use alloc::{string::String, vec::{Vec}};
use gluon_parser::ast::Publicity;
use hashbrown::HashMap;

/// All possible kinds of bindings in Fermion3
#[derive(Debug, Clone)]
pub enum BindingKind {

    // Local-level bindings

    /// A parameter to some function
    Parameter,

    /// Some local binding in the function (e.g let..)
    Local { is_mutable: bool },

    // Module-level bindings

    /// A global let which can be an exported value (e.g let grr =...)
    Let { is_mutable: bool, publicity: Publicity },

    /// A function (e.g fn blah..)
    Function { id: FunctionId, publicity: Publicity },

    /// A type definition/alias (e.g type Bleh = Meow...)
    Type { publicity: Publicity },

    /// A macro definition/alias (e.g macro fn blah...)
    Macro { publicity: Publicity },
    
    /// An imported module object (e.g. `import "math.f3" as m`)
    Import { path: String }, 
}

/// A unique ID representing some binding
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BindingId(pub usize);

/// A unique ID representing some function
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FunctionId(pub usize);

/// One binding of something
/// 
/// This binding has a unique `id` to represent it
/// so that it can be referenced easily.
/// 
/// It has some `name` which is the identifier of this
/// binding
/// 
/// And some `kind` which represents what kind of
/// binding this represents
#[derive(Debug, Clone)]
pub struct Binding {
    pub id: BindingId,
    pub name: String,
    pub kind: BindingKind,
}

/// What kind of boundary is this scope/what is
/// this scoping in?
/// 
/// Either a new `{}` Block scope, a new function scope
/// or at the global module scope level
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeBoundary {
    Block,
    Function(FunctionId),
    Module,
}

/// One scope which can contain many bindings with a
/// mapping from some identifier to it's actual BindingID
#[derive(Debug, Clone)]
pub struct Scope {
    /// The parent scope, this is some index into the scope tree
    pub parent: Option<usize>,
    pub boundary: ScopeBoundary,
    pub bindings: HashMap<String, BindingId>,
}

/// The full tree of scopes, this contains the
/// fully resolved scope tree for all `Bindings`
/// in a `Module`
/// 
/// This is used for holding which actual `BindingID`
/// an `AstNode` refers to, in order to handle shadowing
/// and various other scope related things
#[derive(Debug)]
pub struct ScopeTree {
    // All scopes, we hold this in a vec to be able to
    // refer to them by some usize index
    pub scopes: Vec<Scope>,
    pub current_scope_idx: usize,
    pub bindings: HashMap<BindingId, Binding>,
    next_binding_id: usize,
}

impl ScopeTree {
    /// Creates a new scope tree with
    /// a root `Scope` for the `Module` level
    pub fn new() -> Self {
        // The first scope is the root one for the entire module
        let root = Scope {
            parent: None,
            boundary: ScopeBoundary::Module,
            bindings: HashMap::new(),
        };

        let mut scopes = Vec::new();
        scopes.push(root);

        Self {
            scopes,
            current_scope_idx: 0,
            bindings: HashMap::new(),
            next_binding_id: 0,
        }
    }

    /// Enter a new scope where the boundary/type of this scope
    /// is `boundary`
    /// 
    /// This will allocate a new scope and set the current scope
    /// to point to that new scope
    pub fn enter_scope(&mut self, boundary: ScopeBoundary) {
        let new_idx = self.scopes.len();
        let new_scope = Scope {
            parent: Some(self.current_scope_idx),
            boundary,
            bindings: HashMap::new(),
        };
        self.scopes.push(new_scope);
        self.current_scope_idx = new_idx;
    }

    /// Exit the current scope, returning to it's parent scope.
    pub fn exit_scope(&mut self) {
        if let Some(parent_idx) = self.scopes[self.current_scope_idx].parent {
            self.current_scope_idx = parent_idx;
        }
    }

    /// Defines a new Binding in the current scope with some `name` of some `kind`
    /// 
    /// Returns the ID of this binding.
    pub fn define(&mut self, name: String, kind: BindingKind) -> BindingId {
        let id = BindingId(self.next_binding_id);
        self.next_binding_id += 1;
        
        let binding = Binding { id, name: name.clone(), kind };

        // All bindings have a unique id that we can lookup in the scope tree
        self.bindings.insert(id, binding);
        
        // Insert into current active scope for the name to the ID
        // so we can lookup names in the future in the current scope
        self.scopes[self.current_scope_idx].bindings.insert(name, id);
        id
    }

    /// Returns the binding for a BindingID
    pub fn lookup_binding(&self, binding_id: &BindingId) -> Option<&Binding> {
        self.bindings.get(binding_id)
    }

    /// Looks up a name, starting from the current scope
    /// and moving up its parents until found or the root is reached.
    ///
    /// Returns the BindingId and the index of the scope where it was found.
    pub fn resolve_name(&self, name: &str) -> Option<(BindingId, usize)> {
        // Start from the current scope
        let mut current_idx = self.current_scope_idx;

        loop {
            let scope = &self.scopes[current_idx];

            // Binding exists in this scope
            if let Some(binding_id) = scope.bindings.get(name) {
                return Some((*binding_id, current_idx));
            }

            // Move up to parent
            if let Some(parent_idx) = scope.parent {
                current_idx = parent_idx;
            } else {
                return None;
            }
        }
    }
}