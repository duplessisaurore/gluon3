//! Types for all the parser Module & Ast types produced by the `Gluon3` parser

use core::fmt::Display;

use alloc::{boxed::Box, rc::Rc};
use alloc::string::String;
use alloc::vec::Vec;
use gluon_debug::{Located, SourceFile};

/// The root node of a single `Fermion3` source file's parsed output
/// 
/// This contains all the AstNodes
#[derive(Debug, Clone, PartialEq)]
pub struct Module<FileName: Display + Clone + PartialEq> {
    /// The file path or identifier for this module
    /// 
    /// This is an Rc<> the same as all other SourceLocation's
    /// to share it around instead.
    pub name: Rc<SourceFile<FileName>>,

    /// All `import` statements found at the top level.
    /// 
    /// This will be scanned in the import-resolution phase
    pub imports: Vec<AstNode<FileName>>,

    /// Top-level `type` declarations
    pub types: Vec<AstNode<FileName>>,

    /// Top-level `fn` declarations
    pub functions: Vec<AstNode<FileName>>,

    /// Top-level `macro fn` declarations, we keep
    /// this seperate from `functions` because macros
    /// are inherently compile time as opposed to run time.
    pub macros: Vec<AstNode<FileName>>,

    /// Any executable top-level expressions
    /// 
    /// This is such as global `let` bindings 
    /// or various other AstNodes at the top level
    pub statements: Vec<AstNode<FileName>>,
}

/// A located ExprKind
/// 
/// This essentially just tracks the source location
/// of some component of the AST
pub type AstNode<FileName> = Located<ExprKind<FileName>, FileName>;

/// Patterns are a sort of mini-DSL inside of `Fermion3` for matching,
/// binding and destructuring but they also need location info!
pub type PatternNode<FileName> = Located<Pattern<FileName>, FileName>;

/// Type parameters used where parametric types are
pub type TypeParams<FileName> = Vec<TypeParam<FileName>>;

/// A literal value, shared between `ExprKind::Lit` (evaluated) and
/// `Pattern::Lit` (matched against).
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Int(i64),
    UInt(u64),
    Float(f64),
    Bool(bool),
    Unit,
    Str(String),
}

/// The publicity of an element
/// 
/// This is mainly for future proofing
/// if we ever wanted pub module or something.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum Publicity {
    Public,
    Private,
}

/// A named field paired with some payload.
///
/// This is a shared shape for many constructs
/// which are all aliases of this with differing payloads.
#[derive(Debug, Clone, PartialEq)]
pub struct Field<Payload> {
    pub name: String,
    pub payload: Payload,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind<FileName: Display + Clone + PartialEq> {
    // = Literals =
    // These are direct mappings from the
    // `Lexer` produced `Tokens`

    /// An Int, UInt, Float, Bool, Unit, or Str literal
    Lit(Literal),

    /// A string containing interpolated segments
    /// 
    /// This is a flat map of `Lit(Literal::Str(...))`'s as the actual
    /// textual nodes and then the expressions making
    /// up the rest of the StrInterp
    StrInterp(Vec<AstNode<FileName>>),

    /// A raw identifier
    Identifier(String),

    // = Data Structure Literals =

    /// An array literal which is some open bracket
    /// `[` followed by each element with a comma if
    /// there's another element and then a closing `]`
    /// 
    // e.g [1.0, 2.0, 3.0, 4.0]
    ArrayLiteral(Vec<ArrayElement<FileName>>),

    /// An object literal which is some open brace
    /// `{` followed by each fields name and then value
    /// seperated with a `:` and then commas to delimit
    /// each field: value pair before ending with a closing
    /// `}`
    /// 
    /// e.g. Point { x: 1.0, y: 2.0, ...existing }
    ObjectLiteral {
        /// Though object literals require types
        /// we can cast the untyped object literal
        /// after
        ///
        /// so this is for the inline type
        /// right before the actual object literal.
        target_type: Option<Box<AstNode<FileName>>>,
        elements: Vec<ObjectElement<FileName>>, 
    },

    /// An enum variant literal, this is the same
    /// as an `ObjectLiteral` but there maybe enum
    /// variants with no actual fields.
    /// 
    /// This could technically be an `ObjectLiteral` but
    /// then the codegen would have to figure out if its
    /// actually an object or an enum later anyway so its
    /// split here to simplify things later.
    /// 
    /// e.g. Shape.Circle { radius: 5.0 }
    EnumVariantLiteral {
        // Shape
        enum_type: Box<AstNode<FileName>>,
        // Circle
        variant_name: String,
        elements: Option<Vec<ObjectElement<FileName>>>,
    },

    // = Blocks and Scopes =

    /// Evaluates to the last expression in the block
    /// 
    /// This is inherently just a list of other expressions.
    Block(Vec<AstNode<FileName>>),

    /// A binding to a local in the current scope
    /// 
    /// this is formed of:
    /// let <if "mut" then is_mutable> <pattern><: annotation> = <initializer>
    LetBinding {
        /// Let bindings can be public in the scope
        publicity: Publicity,

        /// the optional <mut> before the pattern
        is_mutable: bool,
        pattern: PatternNode<FileName>,
        annotation: Option<Box<AstNode<FileName>>>,
        initializer: Box<AstNode<FileName>>,
    },
    
    // = Functions and Type definitions =

    /// A function definition which can either
    /// be at the top level, or as an anonymous closure
    /// and returned as the expression value
    /// 
    /// at the top level the syntax is somewhat like this:
    /// fn <name><type_params>(< params <: param_type >>) < -> return_type > => <body>
    /// 
    /// A closure is the same but without a <name>
    FunctionDef {
        /// Function definitions can be public
        publicity: Publicity,

        /// None for anonymous closures
        name: Option<String>, 

        /// These are type parameters that can be
        /// called as name<Type>(params), do not confuse
        /// with the param types that are annotated!
        type_params: TypeParams<FileName>,

        /// param name (pattern for destructuring)
        /// and then the annotated type
        params: Vec<ValueParam<FileName>>,
        
        /// optional return type by `->`
        return_type: Option<Box<AstNode<FileName>>>,

        /// body of the function yay
        body: Box<AstNode<FileName>>,
    },

    /// A type definition which can either
    /// be an alias of an existing type or a new object/enum
    /// that are the sum and product types (object is product, enum is sum)
    /// 
    /// syntax too long blehhh go look at `Fermion3` specification.
    TypeDef {
        /// Type definitions can be public
        publicity: Publicity,

        /// There are no "anonymous types" like closures, 
        /// so we always have a name 
        name: String,

        /// Type parameters e.g Type<params>
        params: TypeParams<FileName>,

        /// The actual underlying type this is a definition over
        /// this can be a new object/enum or an alias over an existing type
        underlying_type: Box<AstNode<FileName>>,
    },

    /// `BaseType where (guard_closure)`
    TypeGuard {
        base: Box<AstNode<FileName>>,
        guard: Box<AstNode<FileName>>,
    },
    
    /// `BaseType fail (message_closure)`
    TypeFail {
        base: Box<AstNode<FileName>>,
        fail_message: Box<AstNode<FileName>>,
    },
    
    /// `BaseType with { fn ... }`
    TypeWith {
        base: Box<AstNode<FileName>>,
        methods: Vec<AstNode<FileName>>,
    },

    // = Control Flow =

    /// A simple if expression
    /// 
    /// This returns the value of it's branches
    /// syntax in `Fermion3` spec based on the
    /// boolean result of the condition
    If {
        /// `if` <condition>
        condition: Box<AstNode<FileName>>,

        /// `then` <branch>, the then is optional
        /// only for inline, the first block here
        /// is also the then branch
        then_branch: Box<AstNode<FileName>>,
        
        /// The optional else case, this is where
        /// we also build our else ifs as more `If`'s
        else_branch: Option<Box<AstNode<FileName>>>,
    },

    /// A match statement, this is really really powerful
    /// but also really complex.
    /// 
    /// Generally we have a subject (that is being matched)
    /// and then all of the arms that the subject attempts to
    /// match.
    Match {
        /// The `match` <subject>
        subject: Box<AstNode<FileName>>,

        /// All of the arms to validate against
        /// in the match statement.
        arms: Vec<MatchArm<FileName>>,
    },

    // = Loops =

    /// A while loop, loop over condition.
    While {
        condition: Box<AstNode<FileName>>,
        body: Box<AstNode<FileName>>,
    },

    /// A for loop.
    /// 
    /// This is a binding loop over
    /// an iteratable type.
    For {
        pattern: PatternNode<FileName>,
        iterable: Box<AstNode<FileName>>,
        body: Box<AstNode<FileName>>,
    },

    /// Infinite loop
    Loop {
        body: Box<AstNode<FileName>>,
    },

    // = Jumps =

    // Break & Return both return a value out (potentially, or just returns)
    Break(Option<Box<AstNode<FileName>>>),
    Continue,
    Return(Option<Box<AstNode<FileName>>>),

    // Raise always raises a value back to catch
    Raise(Box<AstNode<FileName>>),

    /// A `try` & `catch` statement
    /// 
    /// The `try` {} block will run some code
    /// that may `raise`, any code that can `raise`
    /// will then be caught by the `catch` block with
    /// the raised error.
    TryCatch {
        try_body: Box<AstNode<FileName>>,
        error_binding: PatternNode<FileName>, 
        catch_body: Box<AstNode<FileName>>,
    },

    /// Defers the execution of some expression
    /// until the function returns or raises an
    /// error.
    /// 
    /// This should execute on any possible
    /// raise/return in the scope of the function
    /// from when this defer was "bound"
    Defer(Box<AstNode<FileName>>),

    // = Type Operations =

    /// Explicit cast of an expression's
    /// value to be a certain type
    /// 
    /// `expr as <target_type>`.
    TypeCast {
        expr: Box<AstNode<FileName>>,
        target_type: Box<AstNode<FileName>>,
    },

    /// Checking type of an expression
    /// only and returning the boolean result
    /// of whether or not it "is" a type.
    /// 
    /// `value is Type`
    TypeCheck {
        expr: Box<AstNode<FileName>>,
        target_type: Box<AstNode<FileName>>,
    },

    // = Built-in operators and assignment =

    /// Simple rebinding of a mutable variable
    /// 
    /// `<target> = <value>`
    Assignment {
        target: Box<AstNode<FileName>>, 
        value: Box<AstNode<FileName>>,
    },  

    /// Compound rebinding of a mutable variable
    /// 
    /// This automatically runs an operation on the rebind
    /// as follows:
    /// 
    /// `<target> <op>= <value>`
    /// 
    /// such that:
    /// 
    /// `<target> = <target> <op> <value>`
    CompoundAssignment {
        op: Box<AstNode<FileName>>,
        target: Box<AstNode<FileName>>,
        value: Box<AstNode<FileName>>,
    },
    
    /// A binary operation of some operand
    /// on some left and right expression value
    /// 
    /// This requires careful left folding such that the
    /// automatic folding permits same-operators without needing
    /// parenthesis, but whenever mixing then we need to require
    /// parenthesis
    BinaryOp {
        op: Box<AstNode<FileName>>,
        left: Box<AstNode<FileName>>,
        right: Box<AstNode<FileName>>,
    },

    /// A simple unary operation
    UnaryOp {
        op: Box<AstNode<FileName>>,
        expr: Box<AstNode<FileName>>,
    },

    /// The pipeline operator |>
    /// 
    /// which consists of the lvalue which is then
    /// passed as the first argument into the right value
    /// or as the `_` binding
    Pipeline {
        left: Box<AstNode<FileName>>,
        right: Box<AstNode<FileName>>,
    },

    /// The `_` placeholder for pipelines: `x |> f(_)`
    Placeholder,

    /// A call of some function with some arguments
    Call {
        callee: Box<AstNode<FileName>>,
        arguments: Vec<AstNode<FileName>>,
    },

    /// Immutable access of a field from an expression
    FieldAccess {
        expr: Box<AstNode<FileName>>,
        field: String,
    },

    /// Index accessing an expression
    IndexAccess {
        expr: Box<AstNode<FileName>>,
        index: Box<AstNode<FileName>>,
    },

    /// A slice into an array
    Slice {
        array: Box<AstNode<FileName>>,
        start: Option<Box<AstNode<FileName>>>,
        end: Option<Box<AstNode<FileName>>>,
    },

    /// A parameterised type with type arguments
    /// passed to the type
    /// 
    /// This is of the form Target<Arguments>
    Parametric {
        target: Box<AstNode<FileName>>,
        arguments: Vec<AstNode<FileName>>,
    },

    /// A function type which consits on the left hand
    /// of the parameter types and on the right hand the
    /// return type of the function
    /// 
    /// ParamTypes -> ReturnType
    FunctionType {
        params: Vec<AstNode<FileName>>,
        return_type: Box<AstNode<FileName>>,
    },

    /// `object { x: Float, y: Float }`
    /// 
    /// This is used for declarations of new objects
    ObjectType {
        fields: Vec<ObjectFieldDef<FileName>>,
    },

    /// `enum { Circle { radius: Float }, Point }`
    /// 
    /// This is used for declarations of new enums
    EnumType {
        variants: Vec<EnumVariantDef<FileName>>,
    },

    // = Macros =

    /// Defines a new macro
    /// 
    /// A macro is essentially just a compile time
    /// function that takes the source code and outputs
    /// new source code !! yayy macros
    MacroDef {
        // See `FunctionDef` for the field meanings
        publicity: Publicity,
        name: String,
        params: Vec<ValueParam<FileName>>,
        body: Box<AstNode<FileName>>,
    },

    /// Invoke a macro
    /// 
    /// This is special and different to `Call`
    /// as it is entirely at compile time.
    MacroInvoke {
        macro_target: Box<AstNode<FileName>>, 
        arguments: Vec<AstNode<FileName>>,
    },

    /// Macro quote, which inserts the elements
    /// back into the code after running macros as
    /// actual AST elements for compilation.
    MacroQuote(Box<AstNode<FileName>>),

    /// Splicing variables into the macro, similar
    /// to that of string interpolation but macros.
    MacroSplice(Box<AstNode<FileName>>),

    /// An unhygienic identifier which exports the
    /// name to the call site, this is an identifier
    /// prefixed with a `#`
    UnhygienicIdentifier(String),

    // = Imports =

    /// An import is essentially just
    /// 
    /// import <path> <as alias>
    /// 
    /// The alias only really makes sense as a String
    /// here
    Import {
        path: String,
        alias: Option<String>,
    }
}

/// The type of a { field : value, etc. } alias
/// as it's quite complex and repeated for Enums & Objects which
/// we don't want more coupling
///
/// In this case it's a vec because we can have zero or more fields,
/// each field is required with some name, but the payload is optional
/// as in a pattern we can optionally rebind or destructure the field more.
pub type PatternObjectLikeFields<FileName> = Vec<Field<Option<PatternNode<FileName>>>>;

/// A possible pattern/destructuring pattern
/// 
/// This is all the ways that during binding
/// destructuring can occur and in match statements
/// the matching/binding.
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern<FileName: Display + Clone + PartialEq> {
    Wildcard,
    Identifier(String),
    Array {
        /// Matched against leading elements
        before: Vec<PatternNode<FileName>>,

        /// Matched between before and after elements
        rest: Option<Box<PatternNode<FileName>>>,

        /// Matched against the trailing elements
        after: Vec<PatternNode<FileName>>,

        // In `[first, ...middle, last]`
        // the `before` is first,
        // the `rest` is middle,
        // the `after` is last
    },
    Object {
        target_type: Option<Box<AstNode<FileName>>>,
        fields: PatternObjectLikeFields<FileName>,
    },
    EnumVariant {
        enum_type: Box<AstNode<FileName>>,
        variant_name: String,
        fields: Option<PatternObjectLikeFields<FileName>>,
    },

    /// A quoted AST section that we attempt to match in a macro `match`.
    /// `MacroSplice` nodes inside the quote act as capturing sub-patterns
    /// rather than normal splicing similar to other match patterns.
    /// 
    /// See Pattern Matching in Macros in the `Fermion3` specification.
    Quote(Box<AstNode<FileName>>),

    /// We can also bind to an unhygienic identifier in macros etc which
    /// is a `Pattern`:
    /// 
    /// let #it = $(cond)
    UnhygienicIdentifier(String),

    /// An Int, UInt, Float, Bool, Unit, or String literal
    /// 
    /// This is for matching directly against a value as the pattern
    Lit(Literal),
}

/// One arm of a match statement
/// 
/// This consists of a <pattern> <guard> => <body>,
#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm<FileName: Display + Clone + PartialEq> {
    /// The pattern to match/destructure on valid match
    pub pattern: PatternNode<FileName>,

    /// Extra guard for testing if this arm's body should be chosen
    /// 
    /// This is the `if n > 0` part found in the specification.
    pub guard: Option<Box<AstNode<FileName>>>,
    pub body: Box<AstNode<FileName>>,
}

/// An element inside an array literal
#[derive(Debug, Clone, PartialEq)]
pub enum ArrayElement<FileName: Display + Clone + PartialEq> {
    Normal(AstNode<FileName>),
    Spread(AstNode<FileName>),
}

/// An element inside an object/enum literal
#[derive(Debug, Clone, PartialEq)]
pub enum ObjectElement<FileName: Display + Clone + PartialEq> {
    // This is different from `ObjectFieldDef` because
    // here the right value is actually a value instead of
    // being the type definition of the field.
    Field(ObjectElementField<FileName>),
    Spread(AstNode<FileName>),
}

/// Represents a variant definition inside an `enum { ... }` type declaration
#[derive(Debug, Clone, PartialEq)]
pub struct EnumVariantDef<FileName: Display + Clone + PartialEq> {
    pub name: String,
    // None means it's a tag-only variant like or `Option.None`
    pub fields: Option<Vec<ObjectFieldDef<FileName>>>,
}

/// One value parameter with a name to some AstNode
/// 
/// This is for places which bind some value and are
/// not types as patterns are used in the name
/// for destructuring capabilities
#[derive(Debug, Clone, PartialEq)]
pub struct ValueParam<FileName: Display + Clone + PartialEq> {
    pub name: PatternNode<FileName>,
    pub annotation: Option<Box<AstNode<FileName>>>
}

/// One type parameter with a name to some AstNode
/// 
/// This is for places which bind some type and
/// require constraints on that type
pub type TypeParam<FileName> = Field<Option<Box<AstNode<FileName>>>>;

/// An object field with the rvalue as the
/// type definition of this field
pub type ObjectFieldDef<FileName> = Field<Box<AstNode<FileName>>>;
 
/// An object field with the rvalue as the actual
/// value here of the field rather than the definition
pub type ObjectElementField<FileName> = Field<Box<AstNode<FileName>>>;
