//! # dllb-graph
//!
//! Native graph model for dllb.
//!
//! Edges are stored as KV pairs with bidirectional keys:
//! - Outgoing: `ns\0db\0table\0~src\0edge_type\0dst` -> edge properties
//! - Incoming: `ns\0db\0table\0~dst\0edge_type_rev\0src` -> empty
//!
//! Traversal is a prefix scan over the sorted keyspace. Multi-hop queries
//! like `A->knows->B->likes->C` chain sequential prefix scans.
//!
//! Supports BFS/DFS traversal, shortest path (Dijkstra/BFS), filtered
//! paths, and basic graph pattern matching.
