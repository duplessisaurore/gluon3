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

#![feature(try_find)]

use clap::Parser;
use gluon_debug::SourceFile;
use gluon_lexer::Lexer;
use gluon_module_resolver::{LoadModule, ModuleLoader};
use gluon_parser::Parser as GluonParser;
use path_absolutize::Absolutize;
use std::{error::Error, fs, io, path::PathBuf, process, string::FromUtf8Error};

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

    /// Included directories for other files
    #[arg(short, long)]
    include: Vec<PathBuf>,
}

pub struct StdLoader {
    cwd: PathBuf,
    included_dirs: Vec<PathBuf>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();

    let input_path = &cli.input;
    let _output_path = &cli.output;
    let included_dirs = cli.include;

    // Read source file
    let source = fs::read_to_string(input_path).unwrap_or_else(|e| {
        eprintln!("error reading {}: {e}", input_path.display());
        process::exit(1);
    });

    let lexer = Lexer::new(
        &source,
        SourceFile {
            filename: input_path.display().to_string(),
        }
        .into(),
    );

    // Lex Fermion3 source
    let tokens = lexer.lex_all_tokens().unwrap_or_else(|e| {
        eprintln!("lex error: {:?}", e);
        process::exit(1);
    });

    // Parse fermion3 source
    let mut parser = GluonParser::new(
        tokens,
        SourceFile {
            filename: input_path.display().to_string(),
        }
        .into(),
    );

    let ast = parser.parse_module().unwrap_or_else(|e| {
        eprintln!("parse error: {:?}", e);
        process::exit(1);
    });

    // Load all dependent modules
    let loader = ModuleLoader::new(
        ast,
        StdLoader {
            cwd: std::env::current_dir()?,
            included_dirs,
        },
    );

    let resolved_graph = loader.resolve_modules().unwrap_or_else(|e| {
        eprintln!("module resolving error: {:?}", e);
        process::exit(1);
    });

    println!("{:#?}", resolved_graph);

    Ok(())
}

/// Errors that can occur while resolving a source file
#[derive(Debug)]
pub enum ResolveModuleError {
    /// An error that comes up from trying to clean up the path
    CleanPathError { error: CleanPathError },

    /// An error that occured while trying to test if a file exists
    /// where it does but some other weird error occured that we should
    /// let the user know about
    TestingFileExists { error: io::Error },
}

impl LoadModule<String> for StdLoader {
    type ResolveSourceError = ResolveModuleError;

    fn resolve_source_file<'path>(
        &mut self,
        path: &'path str,
    ) -> Result<SourceFile<String>, Self::ResolveSourceError> {
        let this_path = PathBuf::from(path);

        // Check if this simply exists in the included dirs
        Ok(
            match self
                .included_dirs
                .iter()
                .try_find(|included_dir| {
                    // Push this path and see if it exists..
                    let mut test_included = (*included_dir).clone();
                    test_included.push(this_path.clone());


                    test_included
                        .try_exists()
                        .map_err(|error| ResolveModuleError::TestingFileExists { error })
                })
                .map(|path| {
                    // Since we are only using try_find, we need to
                    // re-map this_path onto the valid path before handling
                    // it if it exists at all.
                    if let Some(path) = path {
                        let mut valid_path = path.clone();
                        valid_path.push(this_path.clone());
                        return Some(valid_path);
                    }
                    path.cloned()
                })? {
                Some(file_path) => {
                    SourceFile {
                        filename: file_path.to_string_lossy().to_string(),
                    }
                }

                // Absolutize and assume it must be in the current dir
                // since it didnt exist in any of our included dirs
                None => {
                    let mut cleaned_path = self.cwd.clone();
                    cleaned_path.push(this_path);

                    // Clean up the path to clean up all .., ~
                    let cleaned_path = cleaned_path
                        .clean_path()
                        .map_err(|error| ResolveModuleError::CleanPathError { error })?;

                    // And now this is our source file
                    SourceFile {
                        filename: cleaned_path.to_string_lossy().to_string(),
                    }
                }
            },
        )
    }

    fn load_module_from_path<'path>(&mut self, path: &'path SourceFile<String>) -> Option<String> {
        // Read its path..
        let path = PathBuf::from(&path.filename);

        // Read file, any error means the module basically cant be loaded.
        fs::read_to_string(path).ok()
    }
}

/// Errors that can occur when cleaning up
/// a certain path
#[derive(Debug)]
pub enum CleanPathError {
    /// Converting the cleaned path back to a UTF8 PathBuf
    /// failed with an error
    UTF8ConversionError { error: FromUtf8Error },

    /// An IO error occured
    IOError { error: io::Error },
}

/// Cleanup paths fully for loading modules
pub trait CleanPath {
    fn clean_path(&self) -> Result<PathBuf, CleanPathError>;
}

impl CleanPath for PathBuf {
    fn clean_path(&self) -> Result<PathBuf, CleanPathError> {
        let path_str = self.to_string_lossy();

        // If the path contains a tilde (~), handle expansion.
        let expanded_path = if path_str.contains('~') {
            // Find the last tilde position to expand only the relevant bits
            // (no point exapnding ~/~/meow per tidle).
            if let Some(last_tilde_pos) = path_str.rfind('~') {
                let from_tilde = &path_str[last_tilde_pos..];
                let expanded = tilde_expand::tilde_expand(from_tilde.as_bytes());
                PathBuf::from(
                    String::from_utf8(expanded)
                        .map_err(|error| CleanPathError::UTF8ConversionError { error })?,
                )
            } else {
                PathBuf::from(&*path_str)
            }
        } else {
            // No tilde, use the original reference as the tilde expanded path
            self.as_path().to_path_buf()
        };

        // Convert to an absolute path.
        let absolute = expanded_path
            .absolutize()
            .map_err(|error| CleanPathError::IOError { error })?;

        Ok(absolute.into_owned())
    }
}
