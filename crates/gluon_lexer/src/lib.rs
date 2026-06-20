//! `Gluon3` is an experimental free and open-source compiler for the `Fermion3`
//! language that translates `Fermion3` into the more textual assembly language `Quark3`.
//!
//! Check out the [repository README](https://github.com/duplessisaurore/gluon3/blob/main/README.md)
//! for more information about the project and join the [Discord](https://discord.gg/wXzj2cqZ3Q) for
//! any discussion.
//!
//! ## Gluon3 Lexer
//!
//! The `lexer` crate provides the lexer that transforms the textual
//! `Fermion3` source code described by the specification into a tokenised
//! form for easier parsing.

#![warn(clippy::pedantic)]
#![no_std]

extern crate alloc;

/// The output of the lexer, the tokens themselves.
///
/// This defines all the token types
pub mod tokens;

/// The lexer type which can be used to produce the tokens
/// described.
///
/// This takes in the textual string input and produces
/// the tokens as the output in a list.
pub mod lexer;
