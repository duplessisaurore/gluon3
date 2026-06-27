//! `Gluon3` is an experimental free and open-source compiler for the `Fermion3`
//! language that translates `Fermion3` into the more textual assembly language `Quark3`.
//!
//! Check out the [repository README](https://github.com/duplessisaurore/gluon3/blob/main/README.md)
//! for more information about the project and join the [Discord](https://discord.gg/wXzj2cqZ3Q) for
//! any discussion.
//!
//! ## Gluon3 Bindings
//!
//! The `bindings` crate provides the resolver that attempts to resolve
//! each identifier to it's binding for all used identifiers in the language
//! and all other locations where the identifier was used too

#![warn(clippy::pedantic)]
#![no_std]

extern crate alloc;

/// Bindings, scopes and scope trees for
/// handling name resolution across scopes
pub mod bindings;

/// The binding resolver which walks the parser
/// outputted AST and resolves each name to its
/// corresponding binding.
pub mod resolver;

/// Errors that can occur during the name
/// resolution process
pub mod errors;
pub use errors::BindingResolveErrorKind;
pub use errors::CrossModuleError;

/// Trait for simplifying paths down to the
/// actual binding name for imports
pub mod binding_trait;

// This is required for the phase to run
pub use binding_trait::PathSimplifier;

/// The cross-module resolution handler
/// which takes a `ResolvedGraph` and runs
/// the `BindingResolver` on each module
pub mod cross_module_resolver;

// This is the main export/class of this phase
pub use cross_module_resolver::CrossModuleBindingResolver;
pub use cross_module_resolver::CrossModuleResolutionMap;

/// Builtin names handling so that they dont
/// go as `UnresolvedNames` and stay happy :D
pub mod builtins;
pub use builtins::Builtins;