//! MetaAST node types -- Rust-native representation of the metastatic
//! meta-model specification (METAST_SPEC.md).
//!
//! Every MetaAST node is a uniform 3-element structure:
//! `{type_atom, keyword_meta, children_or_value}`
//!
//! This module mirrors that as `MetaNode { node_type, meta, children }`.

/// The meta-modeling layer a node type belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    /// M2.1: Universal concepts present in all languages.
    Core,
    /// M2.2: Common patterns present in most languages.
    Extended,
    /// M2.2s: Structural/organizational constructs.
    Structural,
    /// M2.3: Language-specific escape hatch.
    Native,
    /// Special wildcard pattern.
    Special,
}

/// All MetaAST node types from the specification.
///
/// Covers M2.1 Core (19 types), M2.2 Extended (14 types),
/// M2.2s Structural (11 types), M2.3 Native (1 type), and Special (1 type).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeType {
    // -- M2.1 Core (all languages) --
    Literal,
    Variable,
    BinaryOp,
    UnaryOp,
    FunctionCall,
    Conditional,
    EarlyReturn,
    Throw,
    Block,
    List,
    Map,
    Pair,
    Tuple,
    Assignment,
    InlineMatch,
    Range,
    StringInterpolation,
    BinSegment,
    Comment,

    // -- M2.2 Extended (most languages) --
    Loop,
    Lambda,
    CollectionOp,
    PatternMatch,
    MatchArm,
    ExceptionHandling,
    AsyncOperation,
    Yield,
    Comprehension,
    Generator,
    Filter,
    Pipe,
    Pin,
    AssertType,

    // -- M2.2s Structural (organizational) --
    Container,
    FunctionDef,
    Param,
    AttributeAccess,
    AugmentedAssignment,
    Property,
    Import,
    TypeAnnotation,
    Decorator,
    RecordUpdate,
    ChildSpec,

    // -- M2.3 Native (language-specific) --
    LanguageSpecific,

    // -- Special --
    /// The `:_` wildcard pattern.
    Wildcard,
}

impl NodeType {
    /// Which meta-modeling layer this node type belongs to.
    pub fn layer(&self) -> Layer {
        match self {
            // Core
            NodeType::Literal
            | NodeType::Variable
            | NodeType::BinaryOp
            | NodeType::UnaryOp
            | NodeType::FunctionCall
            | NodeType::Conditional
            | NodeType::EarlyReturn
            | NodeType::Throw
            | NodeType::Block
            | NodeType::List
            | NodeType::Map
            | NodeType::Pair
            | NodeType::Tuple
            | NodeType::Assignment
            | NodeType::InlineMatch
            | NodeType::Range
            | NodeType::StringInterpolation
            | NodeType::BinSegment
            | NodeType::Comment => Layer::Core,

            // Extended
            NodeType::Loop
            | NodeType::Lambda
            | NodeType::CollectionOp
            | NodeType::PatternMatch
            | NodeType::MatchArm
            | NodeType::ExceptionHandling
            | NodeType::AsyncOperation
            | NodeType::Yield
            | NodeType::Comprehension
            | NodeType::Generator
            | NodeType::Filter
            | NodeType::Pipe
            | NodeType::Pin
            | NodeType::AssertType => Layer::Extended,

            // Structural
            NodeType::Container
            | NodeType::FunctionDef
            | NodeType::Param
            | NodeType::AttributeAccess
            | NodeType::AugmentedAssignment
            | NodeType::Property
            | NodeType::Import
            | NodeType::TypeAnnotation
            | NodeType::Decorator
            | NodeType::RecordUpdate
            | NodeType::ChildSpec => Layer::Structural,

            NodeType::LanguageSpecific => Layer::Native,
            NodeType::Wildcard => Layer::Special,
        }
    }

    /// The Elixir atom name for this node type (for interop with metastatic).
    pub fn atom_name(&self) -> &'static str {
        match self {
            NodeType::Literal => "literal",
            NodeType::Variable => "variable",
            NodeType::BinaryOp => "binary_op",
            NodeType::UnaryOp => "unary_op",
            NodeType::FunctionCall => "function_call",
            NodeType::Conditional => "conditional",
            NodeType::EarlyReturn => "early_return",
            NodeType::Throw => "throw",
            NodeType::Block => "block",
            NodeType::List => "list",
            NodeType::Map => "map",
            NodeType::Pair => "pair",
            NodeType::Tuple => "tuple",
            NodeType::Assignment => "assignment",
            NodeType::InlineMatch => "inline_match",
            NodeType::Range => "range",
            NodeType::StringInterpolation => "string_interpolation",
            NodeType::BinSegment => "bin_segment",
            NodeType::Comment => "comment",
            NodeType::Loop => "loop",
            NodeType::Lambda => "lambda",
            NodeType::CollectionOp => "collection_op",
            NodeType::PatternMatch => "pattern_match",
            NodeType::MatchArm => "match_arm",
            NodeType::ExceptionHandling => "exception_handling",
            NodeType::AsyncOperation => "async_operation",
            NodeType::Yield => "yield",
            NodeType::Comprehension => "comprehension",
            NodeType::Generator => "generator",
            NodeType::Filter => "filter",
            NodeType::Pipe => "pipe",
            NodeType::Pin => "pin",
            NodeType::AssertType => "assert_type",
            NodeType::Container => "container",
            NodeType::FunctionDef => "function_def",
            NodeType::Param => "param",
            NodeType::AttributeAccess => "attribute_access",
            NodeType::AugmentedAssignment => "augmented_assignment",
            NodeType::Property => "property",
            NodeType::Import => "import",
            NodeType::TypeAnnotation => "type_annotation",
            NodeType::Decorator => "decorator",
            NodeType::RecordUpdate => "record_update",
            NodeType::ChildSpec => "child_spec",
            NodeType::LanguageSpecific => "language_specific",
            NodeType::Wildcard => "_",
        }
    }

    /// Parse from an Elixir atom name string.
    pub fn from_atom(name: &str) -> Option<Self> {
        match name {
            "literal" => Some(NodeType::Literal),
            "variable" => Some(NodeType::Variable),
            "binary_op" => Some(NodeType::BinaryOp),
            "unary_op" => Some(NodeType::UnaryOp),
            "function_call" => Some(NodeType::FunctionCall),
            "conditional" => Some(NodeType::Conditional),
            "early_return" => Some(NodeType::EarlyReturn),
            "throw" => Some(NodeType::Throw),
            "block" => Some(NodeType::Block),
            "list" => Some(NodeType::List),
            "map" => Some(NodeType::Map),
            "pair" => Some(NodeType::Pair),
            "tuple" => Some(NodeType::Tuple),
            "assignment" => Some(NodeType::Assignment),
            "inline_match" => Some(NodeType::InlineMatch),
            "range" => Some(NodeType::Range),
            "string_interpolation" => Some(NodeType::StringInterpolation),
            "bin_segment" => Some(NodeType::BinSegment),
            "comment" => Some(NodeType::Comment),
            "loop" => Some(NodeType::Loop),
            "lambda" => Some(NodeType::Lambda),
            "collection_op" => Some(NodeType::CollectionOp),
            "pattern_match" => Some(NodeType::PatternMatch),
            "match_arm" => Some(NodeType::MatchArm),
            "exception_handling" => Some(NodeType::ExceptionHandling),
            "async_operation" => Some(NodeType::AsyncOperation),
            "yield" => Some(NodeType::Yield),
            "comprehension" => Some(NodeType::Comprehension),
            "generator" => Some(NodeType::Generator),
            "filter" => Some(NodeType::Filter),
            "pipe" => Some(NodeType::Pipe),
            "pin" => Some(NodeType::Pin),
            "assert_type" => Some(NodeType::AssertType),
            "container" => Some(NodeType::Container),
            "function_def" => Some(NodeType::FunctionDef),
            "param" => Some(NodeType::Param),
            "attribute_access" => Some(NodeType::AttributeAccess),
            "augmented_assignment" => Some(NodeType::AugmentedAssignment),
            "property" => Some(NodeType::Property),
            "import" => Some(NodeType::Import),
            "type_annotation" => Some(NodeType::TypeAnnotation),
            "decorator" => Some(NodeType::Decorator),
            "record_update" => Some(NodeType::RecordUpdate),
            "child_spec" => Some(NodeType::ChildSpec),
            "language_specific" => Some(NodeType::LanguageSpecific),
            "_" => Some(NodeType::Wildcard),
            _ => None,
        }
    }
}

/// A dynamically-typed metadata value in a MetaAST node.
#[derive(Debug, Clone, PartialEq)]
pub enum MetaValue {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Null,
    Atom(String),
    List(Vec<MetaValue>),
    Node(Box<MetaNode>),
}

/// The children/value of a MetaAST node.
#[derive(Debug, Clone, PartialEq)]
pub enum NodeChildren {
    /// Leaf nodes: the actual value (for :literal, :variable, :comment, :param).
    Value(MetaValue),
    /// Composite nodes: list of child MetaAST nodes.
    Nodes(Vec<MetaNode>),
}

/// A single node in a MetaAST tree.
///
/// Mirrors the Elixir 3-tuple `{type_atom, keyword_meta, children_or_value}`.
#[derive(Debug, Clone, PartialEq)]
pub struct MetaNode {
    pub node_type: NodeType,
    pub meta: Vec<(String, MetaValue)>,
    pub children: NodeChildren,
}

impl MetaNode {
    /// Create a leaf node with a value.
    pub fn leaf(node_type: NodeType, meta: Vec<(String, MetaValue)>, value: MetaValue) -> Self {
        Self {
            node_type,
            meta,
            children: NodeChildren::Value(value),
        }
    }

    /// Create a composite node with children.
    pub fn composite(
        node_type: NodeType,
        meta: Vec<(String, MetaValue)>,
        children: Vec<MetaNode>,
    ) -> Self {
        Self {
            node_type,
            meta,
            children: NodeChildren::Nodes(children),
        }
    }

    /// Get a metadata value by key.
    pub fn get_meta(&self, key: &str) -> Option<&MetaValue> {
        self.meta.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// Get a metadata string value by key.
    pub fn get_meta_str(&self, key: &str) -> Option<&str> {
        match self.get_meta(key) {
            Some(MetaValue::String(s)) => Some(s.as_str()),
            Some(MetaValue::Atom(s)) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Get child nodes (empty vec for leaf nodes).
    pub fn child_nodes(&self) -> &[MetaNode] {
        match &self.children {
            NodeChildren::Nodes(nodes) => nodes,
            NodeChildren::Value(_) => &[],
        }
    }

    /// Get the leaf value (None for composite nodes).
    pub fn leaf_value(&self) -> Option<&MetaValue> {
        match &self.children {
            NodeChildren::Value(v) => Some(v),
            NodeChildren::Nodes(_) => None,
        }
    }

    /// Which meta-modeling layer this node belongs to.
    pub fn layer(&self) -> Layer {
        self.node_type.layer()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_node_types_have_atom_names() {
        // Verify the full 46 node types are covered (45 Metastatic + wildcard).
        let types = [
            NodeType::Literal,
            NodeType::Variable,
            NodeType::BinaryOp,
            NodeType::UnaryOp,
            NodeType::FunctionCall,
            NodeType::Conditional,
            NodeType::EarlyReturn,
            NodeType::Throw,
            NodeType::Block,
            NodeType::List,
            NodeType::Map,
            NodeType::Pair,
            NodeType::Tuple,
            NodeType::Assignment,
            NodeType::InlineMatch,
            NodeType::Range,
            NodeType::StringInterpolation,
            NodeType::BinSegment,
            NodeType::Comment,
            NodeType::Loop,
            NodeType::Lambda,
            NodeType::CollectionOp,
            NodeType::PatternMatch,
            NodeType::MatchArm,
            NodeType::ExceptionHandling,
            NodeType::AsyncOperation,
            NodeType::Yield,
            NodeType::Comprehension,
            NodeType::Generator,
            NodeType::Filter,
            NodeType::Pipe,
            NodeType::Pin,
            NodeType::AssertType,
            NodeType::Container,
            NodeType::FunctionDef,
            NodeType::Param,
            NodeType::AttributeAccess,
            NodeType::AugmentedAssignment,
            NodeType::Property,
            NodeType::Import,
            NodeType::TypeAnnotation,
            NodeType::Decorator,
            NodeType::RecordUpdate,
            NodeType::ChildSpec,
            NodeType::LanguageSpecific,
            NodeType::Wildcard,
        ];
        assert_eq!(types.len(), 46);
        for t in &types {
            let name = t.atom_name();
            assert!(!name.is_empty());
            assert_eq!(NodeType::from_atom(name), Some(*t));
        }
    }

    #[test]
    fn layer_classification() {
        assert_eq!(NodeType::Literal.layer(), Layer::Core);
        assert_eq!(NodeType::Throw.layer(), Layer::Core);
        assert_eq!(NodeType::Loop.layer(), Layer::Extended);
        assert_eq!(NodeType::Yield.layer(), Layer::Extended);
        assert_eq!(NodeType::Pipe.layer(), Layer::Extended);
        assert_eq!(NodeType::Pin.layer(), Layer::Extended);
        assert_eq!(NodeType::AssertType.layer(), Layer::Extended);
        assert_eq!(NodeType::Container.layer(), Layer::Structural);
        assert_eq!(NodeType::Decorator.layer(), Layer::Structural);
        assert_eq!(NodeType::RecordUpdate.layer(), Layer::Structural);
        assert_eq!(NodeType::ChildSpec.layer(), Layer::Structural);
        assert_eq!(NodeType::LanguageSpecific.layer(), Layer::Native);
        assert_eq!(NodeType::Wildcard.layer(), Layer::Special);
    }

    #[test]
    fn meta_node_construction() {
        // {:binary_op, [category: :arithmetic, operator: :+], [left, right]}
        let left = MetaNode::leaf(NodeType::Variable, vec![], MetaValue::String("x".into()));
        let right = MetaNode::leaf(
            NodeType::Literal,
            vec![("subtype".into(), MetaValue::Atom("integer".into()))],
            MetaValue::Int(5),
        );
        let binop = MetaNode::composite(
            NodeType::BinaryOp,
            vec![
                ("category".into(), MetaValue::Atom("arithmetic".into())),
                ("operator".into(), MetaValue::Atom("+".into())),
            ],
            vec![left, right],
        );

        assert_eq!(binop.node_type, NodeType::BinaryOp);
        assert_eq!(binop.get_meta_str("category"), Some("arithmetic"));
        assert_eq!(binop.get_meta_str("operator"), Some("+"));
        assert_eq!(binop.child_nodes().len(), 2);
        assert_eq!(binop.layer(), Layer::Core);
    }
}
