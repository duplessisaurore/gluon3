//! The actual `Parser` type
//!
//! This translates the tokenised content into a `Module`
//! with the abstract syntax tree `AstNode`'s representing
//! all program bits

use core::fmt::Display;

/// The actual parser class itself
pub struct Parser<'src, FileName: Display + Clone> {
}

impl<'src, FileName: Display + Clone> Parser<'src, FileName> {
    
}