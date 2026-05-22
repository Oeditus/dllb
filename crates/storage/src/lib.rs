//! # dllb-storage
//!
//! KV store abstraction, redb backend, and binary key encoding.
//!
//! redb is a pure-Rust, ACID, crash-safe, embedded key-value store using
//! copy-on-write B-trees. It provides MVCC for concurrent readers and a
//! single writer without blocking.

pub mod actor;
pub mod backend;
pub mod db;
pub mod key;
pub mod kv;
