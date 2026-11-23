use core::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum GraphError {
    /// An edge references a node index >= node_count.
    EdgeOutOfRange {
        edge: usize,
        index: u32,
        node_count: usize,
    },
    /// A numeric attribute contains NaN or infinity.
    NonFinite { field: &'static str, index: usize },
    /// An attribute array does not have one entry per node.
    LengthMismatch {
        field: &'static str,
        expected: usize,
        actual: usize,
    },
    /// Node or adjacency count does not fit in the u32 indices used internally.
    TooLarge { field: &'static str, count: usize },
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GraphError::EdgeOutOfRange {
                edge,
                index,
                node_count,
            } => write!(
                f,
                "edge {edge} references node {index}, but there are only {node_count} nodes"
            ),
            GraphError::NonFinite { field, index } => {
                write!(f, "{field}[{index}] is not a finite number")
            }
            GraphError::LengthMismatch {
                field,
                expected,
                actual,
            } => {
                write!(f, "{field} has {actual} entries, expected {expected}")
            }
            GraphError::TooLarge { field, count } => {
                write!(f, "{field} count {count} exceeds the u32 index limit")
            }
        }
    }
}

impl std::error::Error for GraphError {}
