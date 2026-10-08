//! SpaceHunter core: file tree, treemap layout, scanner, formatting and colours.

pub mod classic;
pub mod fmt;
pub mod layout;
pub mod progress;
pub mod sample;
#[cfg(not(target_arch = "wasm32"))]
pub mod scan;
pub mod tree;

pub use classic::{layout_classic, ClassicOptions, DENSITY};
pub use layout::{hit_test, layout, Cell, CellKind, LayoutOptions};
pub use progress::Progress;
pub use tree::{Node, NodeId, Tree, TreeBuilder, NONE};
