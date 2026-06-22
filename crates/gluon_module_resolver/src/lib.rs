//! `Gluon3` is an experimental free and open-source compiler for the `Fermion3`
//! language that translates `Fermion3` into the more textual assembly language `Quark3`.
//!
//! Check out the [repository README](https://github.com/duplessisaurore/gluon3/blob/main/README.md)
//! for more information about the project and join the [Discord](https://discord.gg/wXzj2cqZ3Q) for
//! any discussion.
//!
//! ## Gluon3 Module Resolver
//!
//! The `module resolver` crate provides the resolver that takes the initial module
//! of `Fermion3` source code produced by the `gluon_parser` into a list of all the
//! resolved modules and dependencies from this initial module

#![warn(clippy::pedantic)]
#![no_std]

extern crate alloc;

/// The load trait, this loads the textual
/// content of the file for us since we inherently
/// cant load it ourselves (no fs/std)
pub mod load_trait;
pub use load_trait::LoadModule;

/// The actual `ModuleLoader` itself, this takes
/// the first `Module` as input and produces
/// one `ResolvedGraph` representing the successful
/// resolved module graph/tree with deps resolved
pub mod resolver;
pub use resolver::ModuleLoader;

/// Error types/result that can occur during module
/// resolving
pub mod errors;
pub use errors::ModuleResolveError;
