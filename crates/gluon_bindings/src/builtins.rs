//! The cross-module resolver
//!
//! This handles inserting all the builtins that should
//! have a `Name` in the binding resolution

/// A single builtin definition with some name
#[derive(Debug)]
pub struct BuiltinDef {
    pub name: &'static str,
}

/// The complete set of primitive bindings to inject
/// per module into it's universe
#[derive(Debug)]
pub struct Builtins(pub &'static [BuiltinDef]);

macro_rules! builtins {
    ($($name:literal),* $(,)?) => {
        Builtins(&[ $( BuiltinDef { name: $name } ),* ])
    };
}

pub static PRIMITIVES: Builtins = builtins![
    // Primitive types
    "Int", "Float", "String", "Bool", "UInt", "Any", "Array", "Never",

    // Arithmetic
    "+", "-", "*", "/", "%", "**", 

    // Comparison
    "==", "!=", "<", ">", "<=", ">=",

    // Boolean
    "&&", "||", "!",

    // Bitwise
    "&", "|", "^", "~"
];