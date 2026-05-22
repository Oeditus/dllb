//! # dllb-document
//!
//! Document model for dllb.
//!
//! This crate provides CRUD operations (CREATE, READ, UPDATE, DELETE) over
//! the KV store, MessagePack serialization of document values, secondary
//! B-tree indexes on arbitrary fields, and schema validation for both
//! schemaless and schemafull tables.
//!
//! Documents are stored as KV pairs:
//! - Key: `ns\0db\0table\0*record_id`
//! - Value: MessagePack-serialized `BTreeMap<String, Value>`
