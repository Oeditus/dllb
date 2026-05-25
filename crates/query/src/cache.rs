//! Write-version-counter cache for expensive computed results.
//!
//! # Design
//!
//! Every edge table `(ns, db, table)` carries a monotonic [`WriteVersions`]
//! counter that is incremented on each `RELATE` or edge-delete. Expensive
//! analytics queries (e.g. `GRAPH COMMUNITIES`) consult a [`ComputeCache`]
//! keyed by `(ns, db, table, computation_kind, params, outcome_format)`.
//!
//! A cached entry is valid as long as its stored version equals the current
//! write counter — guaranteeing that any edge mutation automatically
//! invalidates all dependent analytics results.
//!
//! # Threading
//!
//! Both types are designed to be shared via `Arc` across all connection
//! handler tasks. Reads use a shared `RwLock` guard; per-entry updates use
//! an `AtomicU64` so bumping a write version only needs a read-level lock on
//! the outer map.
//!
//! # No stale data guarantee
//!
//! Because the version is read *before* the computation starts and written
//! *after* it completes, a concurrent write that arrives mid-computation
//! merely causes the freshly stored entry to be stale — the next request
//! will see the new version, miss the cache, and recompute. Results are
//! never served that were computed from data older than their stated version.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use crate::ast::OutcomeFormat;

// ---------------------------------------------------------------------------
// WriteVersions
// ---------------------------------------------------------------------------

type TableKey = (String, String, String); // (ns, db, table)

/// Per-`(ns, db, table)` monotonic write counter.
///
/// Incremented on every edge mutation; read before and after analytics
/// computations to determine cache validity.
#[derive(Default)]
pub struct WriteVersions {
    map: RwLock<HashMap<TableKey, Arc<AtomicU64>>>,
}

impl WriteVersions {
    /// Current version for the given table. Returns `0` if the table has
    /// never been written to.
    pub fn current(&self, ns: &str, db: &str, table: &str) -> u64 {
        let key = table_key(ns, db, table);
        self.map
            .read()
            .unwrap()
            .get(&key)
            .map(|c| c.load(Ordering::Acquire))
            .unwrap_or(0)
    }

    /// Increment the write counter for `table` by 1.
    pub fn bump(&self, ns: &str, db: &str, table: &str) {
        let key = table_key(ns, db, table);

        // Fast path: the counter already exists — bump without a write lock.
        {
            let guard = self.map.read().unwrap();
            if let Some(counter) = guard.get(&key) {
                counter.fetch_add(1, Ordering::Release);
                return;
            }
        }

        // Slow path: first write to this table — insert a new counter.
        let mut guard = self.map.write().unwrap();
        // Double-checked: another task may have inserted while we waited for
        // the write lock.
        guard
            .entry(key)
            .or_insert_with(|| Arc::new(AtomicU64::new(0)))
            .fetch_add(1, Ordering::Release);
    }
}

fn table_key(ns: &str, db: &str, table: &str) -> TableKey {
    (ns.into(), db.into(), table.into())
}

// ---------------------------------------------------------------------------
// CacheKind
// ---------------------------------------------------------------------------

/// Identifies which computation a cache entry represents and its parameters.
///
/// Parameters that are `f64` are stored as raw bits (`u64`) so the type
/// can implement `Hash` and `Eq` without floating-point equality hazards.
#[derive(PartialEq, Eq, Hash, Clone, Debug)]
pub enum CacheKind {
    Communities {
        algorithm: String,
        max_iterations: usize,
        /// `f64::to_bits()` of the resolution parameter.
        resolution_bits: u64,
    },
}

impl CacheKind {
    /// Construct a `Communities` key from typed parameters.
    pub fn communities(algorithm: &str, max_iterations: usize, resolution: f64) -> Self {
        CacheKind::Communities {
            algorithm: algorithm.to_string(),
            max_iterations,
            resolution_bits: resolution.to_bits(),
        }
    }
}

// ---------------------------------------------------------------------------
// CacheKey
// ---------------------------------------------------------------------------

/// Full identifier for a single cached computation.
#[derive(Clone, Debug)]
pub struct CacheKey {
    ns: String,
    db: String,
    table: String,
    kind: CacheKind,
    outcome: OutcomeFormat,
}

impl CacheKey {
    pub fn new(
        ns: &str,
        db: &str,
        table: &str,
        kind: CacheKind,
        outcome: OutcomeFormat,
    ) -> Self {
        CacheKey {
            ns: ns.into(),
            db: db.into(),
            table: table.into(),
            kind,
            outcome,
        }
    }
}

impl PartialEq for CacheKey {
    fn eq(&self, other: &Self) -> bool {
        self.ns == other.ns
            && self.db == other.db
            && self.table == other.table
            && self.kind == other.kind
            && self.outcome == other.outcome
    }
}

impl Eq for CacheKey {}

impl Hash for CacheKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.ns.hash(state);
        self.db.hash(state);
        self.table.hash(state);
        self.kind.hash(state);
        // Hash OutcomeFormat by its numeric discriminant.
        (self.outcome as u8).hash(state);
    }
}

// ---------------------------------------------------------------------------
// ComputeCache
// ---------------------------------------------------------------------------

struct CacheEntry {
    /// Pre-formatted wire-ready response string (already in the requested
    /// `OutcomeFormat`).
    payload: String,
    /// Write version of the source table at the time of computation.
    version: u64,
}

/// Thread-safe cache mapping `CacheKey` → pre-formatted response strings.
///
/// Shared via `Arc` across all connection handlers; lookups use a read lock
/// and are non-blocking as long as no insertion is in progress.
#[derive(Default)]
pub struct ComputeCache {
    map: RwLock<HashMap<CacheKey, CacheEntry>>,
}

impl ComputeCache {
    /// Return the cached payload if it was computed at exactly
    /// `current_version`. Returns `None` on a miss or stale entry.
    pub fn get(&self, key: &CacheKey, current_version: u64) -> Option<String> {
        let guard = self.map.read().unwrap();
        guard.get(key).and_then(|e| {
            if e.version == current_version {
                Some(e.payload.clone())
            } else {
                None
            }
        })
    }

    /// Store a computation result alongside the version it was computed at.
    pub fn insert(&self, key: CacheKey, payload: String, version: u64) {
        self.map
            .write()
            .unwrap()
            .insert(key, CacheEntry { payload, version });
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- WriteVersions -------------------------------------------------------

    #[test]
    fn versions_start_at_zero() {
        let v = WriteVersions::default();
        assert_eq!(v.current("ns", "db", "t"), 0);
    }

    #[test]
    fn bump_increments_by_one() {
        let v = WriteVersions::default();
        v.bump("ns", "db", "t");
        assert_eq!(v.current("ns", "db", "t"), 1);
        v.bump("ns", "db", "t");
        assert_eq!(v.current("ns", "db", "t"), 2);
    }

    #[test]
    fn tables_are_isolated() {
        let v = WriteVersions::default();
        v.bump("ns", "db", "calls");
        v.bump("ns", "db", "calls");
        assert_eq!(v.current("ns", "db", "calls"), 2);
        assert_eq!(v.current("ns", "db", "contains"), 0);
    }

    #[test]
    fn namespaces_are_isolated() {
        let v = WriteVersions::default();
        v.bump("ns1", "db", "t");
        assert_eq!(v.current("ns1", "db", "t"), 1);
        assert_eq!(v.current("ns2", "db", "t"), 0);
    }

    #[test]
    fn concurrent_bumps_are_consistent() {
        use std::sync::Arc;
        use std::thread;

        let v = Arc::new(WriteVersions::default());
        let handles: Vec<_> = (0..16)
            .map(|_| {
                let v = Arc::clone(&v);
                thread::spawn(move || {
                    for _ in 0..100 {
                        v.bump("ns", "db", "t");
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(v.current("ns", "db", "t"), 1_600);
    }

    // -- ComputeCache --------------------------------------------------------

    fn key(_version_hint: &str) -> CacheKey {
        CacheKey::new(
            "ns",
            "db",
            "calls",
            CacheKind::communities("louvain", 10, 1.0),
            OutcomeFormat::Json,
        )
    }

    #[test]
    fn cache_miss_when_empty() {
        let c = ComputeCache::default();
        assert!(c.get(&key(""), 0).is_none());
    }

    #[test]
    fn cache_hit_at_matching_version() {
        let c = ComputeCache::default();
        let k = key("");
        c.insert(k.clone(), "payload".into(), 7);
        assert_eq!(c.get(&k, 7), Some("payload".into()));
    }

    #[test]
    fn cache_miss_when_version_stale() {
        let c = ComputeCache::default();
        let k = key("");
        c.insert(k.clone(), "old".into(), 3);
        // Version advanced since cache was built.
        assert!(c.get(&k, 4).is_none());
    }

    #[test]
    fn cache_miss_when_version_newer_than_cached() {
        // Edge case: version went backwards (e.g. server restart). Since
        // the server restart also clears the in-memory cache, this can't
        // happen in practice — but verify the behaviour is a miss anyway.
        let c = ComputeCache::default();
        let k = key("");
        c.insert(k.clone(), "future".into(), 10);
        assert!(c.get(&k, 5).is_none());
    }

    #[test]
    fn different_params_do_not_collide() {
        let c = ComputeCache::default();
        let k1 = CacheKey::new(
            "ns",
            "db",
            "calls",
            CacheKind::communities("louvain", 10, 1.0),
            OutcomeFormat::Json,
        );
        let k2 = CacheKey::new(
            "ns",
            "db",
            "calls",
            CacheKind::communities("lp", 10, 1.0),
            OutcomeFormat::Json,
        );
        c.insert(k1.clone(), "louvain-result".into(), 1);
        assert_eq!(c.get(&k1, 1), Some("louvain-result".into()));
        assert!(c.get(&k2, 1).is_none());
    }

    #[test]
    fn different_outcome_formats_do_not_collide() {
        let c = ComputeCache::default();
        let k_json = CacheKey::new(
            "ns",
            "db",
            "calls",
            CacheKind::communities("louvain", 10, 1.0),
            OutcomeFormat::Json,
        );
        let k_toon = CacheKey::new(
            "ns",
            "db",
            "calls",
            CacheKind::communities("louvain", 10, 1.0),
            OutcomeFormat::Toon,
        );
        c.insert(k_json.clone(), "json-payload".into(), 1);
        assert_eq!(c.get(&k_json, 1), Some("json-payload".into()));
        assert!(c.get(&k_toon, 1).is_none());
    }

    #[test]
    fn insert_overwrites_stale_entry() {
        let c = ComputeCache::default();
        let k = key("");
        c.insert(k.clone(), "v1".into(), 1);
        c.insert(k.clone(), "v2".into(), 2);
        assert!(c.get(&k, 1).is_none()); // old version no longer valid
        assert_eq!(c.get(&k, 2), Some("v2".into()));
    }
}
