//! # dllb-transaction
//!
//! MVCC transaction manager for dllb.
//!
//! Will provide:
//! - Monotonically increasing transaction timestamps
//! - Snapshot isolation: reads see only versions committed before the
//!   transaction's start timestamp
//! - Optimistic concurrency control with conflict detection on commit
//! - Garbage collection of old versions based on a watermark
//!
//! Currently deferred: redb provides built-in MVCC with serializable
//! isolation, which suffices for the prototype.
