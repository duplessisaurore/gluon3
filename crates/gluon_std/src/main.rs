//! `Gluon3` is an experimental free and open-source compiler for the `Fermion3`
//! language that translates `Fermion3` into the more textual assembly language `Quark3`.
//!
//! Check out the [repository README](https://github.com/duplessisaurore/gluon3/blob/main/README.md)
//! for more information about the project and join the [Discord](https://discord.gg/wXzj2cqZ3Q) for
//! any discussion.
//!
//! ## Gluon3 STD
//!
//! The `gluon_std` crate provides a binary for compiling `Fermion3`
//! language files into `Quark3` assembly for systems that support the
//! rust std.

use clap::Parser;
use gluon_debug::SourceFile;
use gluon_lexer::Lexer;
use std::{error::Error, fs, path::PathBuf, process};

#[derive(Parser)]
#[command(
    name = "gluon3",
    about = "Compiles Fermion3 source files into Quark3 assembly"
)]
struct Cli {
    /// Input Fermion3 source file
    input: PathBuf,

    /// Output Quark3 assembly file
    output: PathBuf,
}

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();

    let input_path = &cli.input;
    let _output_path = &cli.output;

    // Read source file
    let source = fs::read_to_string(input_path).unwrap_or_else(|e| {
        eprintln!("error reading {}: {e}", input_path.display());
        process::exit(1);
    });

    let lexer = Lexer::new(
        &source,
        SourceFile {
            filename: input_path.display(),
        }
        .into(),
    );

    // Lex Fermion3 source
    let tokens = lexer.lex_all_tokens().unwrap_or_else(|e| {
        eprintln!("lex error: {:?}", e);
        process::exit(1);
    });

    println!("{:#?}", tokens);
    Ok(())
}
