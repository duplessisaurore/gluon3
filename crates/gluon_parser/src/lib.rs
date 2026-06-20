//! `Gluon3` is an experimental free and open-source compiler for the `Fermion3`
//! language that translates `Fermion3` into the more textual assembly language `Quark3`.
//!
//! Check out the [repository README](https://github.com/duplessisaurore/gluon3/blob/main/README.md)
//! for more information about the project and join the [Discord](https://discord.gg/wXzj2cqZ3Q) for
//! any discussion.
//!
//! ## Gluon3 Parser
//!
//! The `parser` crate provides the parser that transforms the tokenised
//! `Fermion3` source code produced by the `gluon_lexer` into an AST
//! for code generation with.

#![warn(clippy::pedantic)]
#![no_std]

extern crate alloc;