//! # dllb-graph
//!
//! Native graph model for dllb.
//!
//! Edges are stored as KV pairs with bidirectional keys:
//! - Outgoing: `ns\0db\0table\0~src\0>edge_type\0dst` -> MessagePack properties
//! - Incoming: `ns\0db\0table\0~dst\0<edge_type\0src` -> empty (pointer only)
//!
//! The `>` and `<` direction bytes separate outgoing from incoming edges
//! within a vertex's key region, enabling direction-aware prefix scans.
//!
//! Traversal is a prefix scan over the sorted keyspace. Multi-hop queries
//! like `A->knows->B->likes->C` chain sequential prefix scans.

pub mod community;
pub mod components;
pub mod edge;
pub mod store;
pub mod traverse;

pub use community::{Algorithm as CommunityAlgorithm, Communities, Options as CommunityOptions};
pub use community::{detect as detect_communities, detect_weighted as detect_communities_weighted};
pub use components::{Components, connected_components};
pub use edge::Edge;
pub use store::EdgeStore;
pub use traverse::{Direction, HopSpec, Traversal};
