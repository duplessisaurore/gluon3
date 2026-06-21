//! Types for all the token types produced by the `Gluon3` lexer

use alloc::string::String;
use gluon_debug::Located;

/// A token of some kind lexed from the source code provided with
/// location information attached
pub type Token<FileName> = Located<TokenKind, FileName>;

/// A kind of token producable by the `Gluon3` Lexer.
///
/// This supports all the `Fermion3` language features excluding
/// macros.
///
/// Each token is categorised by the first few letters as follows:
///
///     All non-string literals begin with `Lit`.
///     All string-literal related tokens (including interp) start with `Str`
///     All keywords start with `Kw`'
///     All delimiters start with `Del`
///     All macro related (non-keywords) start with `Macro`
///
/// Other elements do not really have a category.
///     
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // = Literals (Lit) =
    /// Integer literals
    /// ------------------
    ///
    /// As per the specification this can come from:
    ///
    /// decimal,
    /// hex (0x),
    /// binary (0b),
    /// octal (0o)
    ///
    /// Stored as i64 after parsing the prefix/base
    LitInt(i64),

    /// Unsigned Integer literals
    /// ------------------
    ///
    /// As per the specification this can come from:
    ///
    /// decimal,
    /// hex (0x),
    /// binary (0b),
    /// octal (0o)
    ///
    /// all followed by a "u", to differentiate from normal
    /// integer literals. This does not support negative numbers.
    ///
    /// Stored as u64 after parsing the prefix/base
    LitUInt(u64),

    /// Floating point literal
    /// ------------------
    ///
    /// As per the specification this can come from:
    ///
    /// decimal.fractional
    /// .fractional
    /// decimal.
    /// any of the above follwoed by an "e<places>"
    ///
    /// Stored as an f64 after parsing the prefix/base.
    LitFloat(f64),

    /// Boolean literal
    /// ------------------
    ///
    /// Allowed:
    /// true / false in any case.
    ///
    LitBool(bool),

    /// Unit literal: ()
    LitUnit,

    // = String Literals (Str) =
    //
    // This needs to support interpolation too which is done through a complex
    // sequence of tokens.
    //
    // A plain string literal should produce:
    // StrStart, StrFragment(literal), StrEnd
    //
    // For example with "hello world":
    // StrStart, StrFragment("hello world"), StrEnd
    //
    // An interpolated string produces a sequence in which
    // consists of StrFragments and then StrInterpStart/StrInterpEnd sequences
    // which contains the normal tokens in the "interpolation region"
    //
    // An example:
    // "hello ${name}, you are ${age} years old"
    //
    // StrStart
    // StrFragment("hello ")
    // StrInterpStart
    // <tokens for name>
    // StrInterpEnd
    // StrFragment(", you are ")
    // StrInterpStart
    // <tokens for age>
    // StrInterpEnd
    // StrFragment(" years old")
    // StrEnd
    //
    /// Opens a string literal
    ///
    /// This should be done when the lexer sees the opening double quote '"'
    StrStart,

    /// A plain text fragment inside a string, between interpolations or
    /// at the start/end. The contained String has escape sequences resolved
    /// (e.g. \n -> newline).
    StrFragment(String),

    /// Opens an interpolation inside a string literal
    ///
    /// This should be done when the lexer sees `${`
    StrInterpStart,

    /// Closes an interpolation
    ///
    /// This should be done when the lexer sees `}` that
    /// terminates an interpolation
    StrInterpEnd,

    /// Opens a string literal
    ///
    /// This should be done when the lexer sees the closing double quote '"'
    StrEnd,

    // = Keywords (Kw) =
    /// Function definition
    KwFn,

    /// Macro function prefix
    KwMacro,

    /// Binding a local
    KwLet,

    /// Declare a binding as mutable
    KwMut,

    /// Bind a new type
    KwType,

    /// Object type
    KwObject,

    /// Enum type
    KwEnum,

    // Control flow
    KwMatch,
    KwIf,
    KwElse,

    /// For element in iterator
    KwFor,

    /// While <condition is true>
    KwWhile,

    /// Infinite loop
    KwLoop,

    KwBreak,
    KwContinue,
    KwReturn,
    KwRaise,
    KwTry,
    KwCatch,

    /// Import another Fermion3 file
    KwImport,

    /// Declare this item as public
    KwPub,

    /// Type cast / Import renaming
    KwAs,

    /// Boolean result of a type check
    KwIs,

    /// For <element> in <iterator>
    KwIn,

    /// Add methods to a type
    /// 
    /// Or in `match <thing> with { arms }`
    KwWith,

    /// If <condition> then {}
    KwThen,

    /// While/for <thingy blah blah> do {}
    KwDo,

    /// Guard for a type
    KwWhere,

    /// Guard message on fail
    KwFail,

    /// Defer an expression to be evaluated later
    KwDefer,

    //  = Identifiers and Operators =
    /// Any identifier that is not a keyword:
    /// variable names, type names, etc.
    Ident(String),

    // = Assignment Operators =

    // Compound assignment consists of some
    // ident ident= ident
    Equal, // =

    // = Special Operators =
    FatArrow,  // =>  (function body / match arm)
    ThinArrow, // ->  (return type annotation / function type)
    PipeArrow, // |>  (pipeline operator)
    DotDot,    // ..  (slice range)
    DotDotDot, // ... (spread operator)

    // = Punctuation =
    Dot,       // .
    Comma,     // ,
    Colon,     // :
    Semicolon, // ;

    // = Delimiters ==
    DelLParen,   // (
    DelRParen,   // )
    DelLBrace,   // {
    DelRBrace,   // }
    DelLBracket, // [
    DelRBracket, // ]

    // Open/close a parametric type delimiter
    // This is context sensitive with the operator Less/Greater
    DelLAngle, // <
    DelRAngle, // >

    // = Macros (Macro) =
    /// Start of a quote in a macro
    ///
    /// ```
    /// ``
    /// ```
    MacroQuoteStart,

    /// End of a quote of a macro
    ///
    /// ```
    /// ``
    /// ```
    MacroQuoteEnd,

    /// Start of a splice in a macro quote
    /// This is like string interpolation but macros
    ///
    /// $(
    MacroSpliceStart,

    /// End of a macro splice in a quote )
    MacroSpliceEnd,

    /// @ For macro invocation compared
    /// to normal function names as idents
    MacroAt,

    /// # For #name hygiene escape in macros
    /// which lets you do
    ///
    /// let #it = $(cond)..
    ///
    /// and then "it" will be actually produced in
    /// the output for the macro
    MacroHash,

    /// End of file
    Eof,
}
