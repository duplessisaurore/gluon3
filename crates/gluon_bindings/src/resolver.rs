use core::fmt::Display;

use alloc::vec::Vec;
use gluon_parser::ast::NodeId;
use hashbrown::HashMap;

use crate::{bindings::{BindingId, FunctionId, ScopeTree}, errors::BindingResolveError};

/// The output of the binding resolution/name resolution phase
/// 
/// This provides all the mappings for the AST tree produced
/// by the `Parser`
#[derive(Debug)]
pub struct BindingResolutionMap {
    /// A mapping of AST `node_id`'s to the `BindingId` if
    /// it resolved to/defined some binding
    pub resolutions: HashMap<NodeId, BindingId>,

    /// The fully built scope tree
    pub scope_tree: ScopeTree,

    /// What bindings each function closes over, by FunctionId.
    pub captures: HashMap<FunctionId, Vec<BindingId>>,
}

/// The actual binding resolver clas itself
pub struct BindingResolver<FileName: Display + Clone + PartialEq> {
    /// The current output resolution map being built
    resolution_map: BindingResolutionMap,

    /// All errors accumulated during binding resolution
    errors: Vec<BindingResolveError<FileName>>,

    /// The last allocated function ID, we increasingly increment this
    /// monotonically to continue having unique FunctionIds's.
    next_function_id: usize,
}