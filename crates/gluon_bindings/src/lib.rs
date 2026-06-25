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