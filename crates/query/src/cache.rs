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
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use lru::LruCache;

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
    /// `GRAPH COMPONENTS` -- connected components (no parameters).
    Components,
    /// `GRAPH PAGERANK` keyed by damping (as raw bits), iteration cap, and the
    /// row limit (the cached payload is already truncated to it).
    PageRank {
        damping_bits: u64,
        max_iterations: usize,
        limit: Option<u64>,
    },
    /// `GRAPH CENTRALITY` keyed by the degree mode name and row limit.
    Centrality { mode: String, limit: Option<u64> },
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

    /// Construct a `PageRank` key from typed parameters.
    pub fn pagerank(damping: f64, max_iterations: usize, limit: Option<u64>) -> Self {
        CacheKind::PageRank {
            damping_bits: damping.to_bits(),
            max_iterations,
            limit,
        }
    }

    /// Construct a `Centrality` key from a mode name and row limit.
    pub fn centrality(mode: &str, limit: Option<u64>) -> Self {
        CacheKind::Centrality {
            mode: mode.to_string(),
            limit,
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
    pub fn new(ns: &str, db: &str, table: &str, kind: CacheKind, outcome: OutcomeFormat) -> Self {
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
    /// Total estimated byte size of this entry (payload string + key overhead).
    bytes: usize,
}

/// Default maximum number of cached query computation results (5,000 entries).
pub const DEFAULT_CACHE_CAPACITY: usize = 5_000;

/// Default maximum memory allocation for cached payloads (1 GB).
pub const DEFAULT_CACHE_MAX_BYTES: usize = 1024 * 1024 * 1024;

struct InnerCache {
    lru: LruCache<CacheKey, CacheEntry>,
    current_bytes: usize,
    max_bytes: usize,
}

/// Thread-safe bounded LRU cache mapping [`CacheKey`] → pre-formatted response strings.
///
/// Shared via `Arc` across all connection handlers; lookups and insertions update
/// recency order under a `Mutex`, evicting the least recently used entry when either
/// max entry count or max byte size is exceeded.
pub struct ComputeCache {
    inner: Mutex<InnerCache>,
}

impl Default for ComputeCache {
    fn default() -> Self {
        Self::from_env()
    }
}

impl ComputeCache {
    /// Create a new `ComputeCache` with item capacity and byte memory limit.
    pub fn new(capacity: usize) -> Self {
        Self::with_bounds(capacity, DEFAULT_CACHE_MAX_BYTES)
    }

    /// Create a new `ComputeCache` specifying both item count limit and byte capacity limit.
    pub fn with_bounds(capacity: usize, max_bytes: usize) -> Self {
        let cap = NonZeroUsize::new(capacity.max(1)).expect("cache capacity must be non-zero");
        Self {
            inner: Mutex::new(InnerCache {
                lru: LruCache::new(cap),
                current_bytes: 0,
                max_bytes,
            }),
        }
    }

    /// Initialize `ComputeCache` from environment variables `DLLB_CACHE_CAPACITY`
    /// (default 5000) and `DLLB_CACHE_MAX_BYTES` (default 1GB, accepts bytes or '1G', '512M').
    pub fn from_env() -> Self {
        let capacity = std::env::var("DLLB_CACHE_CAPACITY")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(DEFAULT_CACHE_CAPACITY);

        let max_bytes = std::env::var("DLLB_CACHE_MAX_BYTES")
            .ok()
            .and_then(|s| parse_bytes_str(&s))
            .unwrap_or(DEFAULT_CACHE_MAX_BYTES);

        Self::with_bounds(capacity, max_bytes)
    }

    /// Return the cached payload if it was computed at exactly
    /// `current_version`. Returns `None` on a miss or stale entry, updating
    /// recency order on a hit.
    pub fn get(&self, key: &CacheKey, current_version: u64) -> Option<String> {
        let mut guard = self.inner.lock().unwrap();
        if let Some(entry) = guard.lru.get(key)
            && entry.version == current_version
        {
            return Some(entry.payload.clone());
        }
        None
    }

    /// Store a computation result alongside the version it was computed at.
    ///
    /// Evicts LRU entries if capacity count or byte limit is exceeded.
    pub fn insert(&self, key: CacheKey, payload: String, version: u64) {
        let entry_bytes = payload.len() + 128; // payload + key overhead estimate
        let mut guard = self.inner.lock().unwrap();

        // If key already exists, subtract old entry bytes before replacing.
        if let Some(old) = guard.lru.pop(&key) {
            guard.current_bytes = guard.current_bytes.saturating_sub(old.bytes);
        }

        // Evict LRU entries while max bytes or item capacity is exceeded.
        while !guard.lru.is_empty()
            && (guard.lru.len() >= guard.lru.cap().get()
                || guard.current_bytes + entry_bytes > guard.max_bytes)
        {
            if let Some((_k, popped)) = guard.lru.pop_lru() {
                guard.current_bytes = guard.current_bytes.saturating_sub(popped.bytes);
            } else {
                break;
            }
        }

        guard.current_bytes += entry_bytes;
        guard.lru.put(
            key,
            CacheEntry {
                payload,
                version,
                bytes: entry_bytes,
            },
        );
    }

    /// Return the current number of cached entries.
    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().lru.len()
    }

    /// Return whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.lock().unwrap().lru.is_empty()
    }

    /// Return total tracked byte consumption of cached entries.
    pub fn current_bytes(&self) -> usize {
        self.inner.lock().unwrap().current_bytes
    }
}

/// Helper to parse human string like "1G", "512M", "100K", or raw numeric bytes.
fn parse_bytes_str(s: &str) -> Option<usize> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(raw) = trimmed.parse::<usize>() {
        return Some(raw);
    }

    let (num_part, unit) = trimmed.split_at(trimmed.len() - 1);
    let num: usize = num_part.trim().parse().ok()?;
    match unit.to_uppercase().as_str() {
        "G" => Some(num * 1024 * 1024 * 1024),
        "M" => Some(num * 1024 * 1024),
        "K" => Some(num * 1024),
        _ => None,
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

    #[test]
    fn lru_evicts_least_recently_used_entry() {
        let c = ComputeCache::new(2);
        let k1 = CacheKey::new(
            "ns",
            "db",
            "t",
            CacheKind::Centrality {
                mode: "degree".into(),
                limit: None,
            },
            OutcomeFormat::Json,
        );
        let k2 = CacheKey::new(
            "ns",
            "db",
            "t",
            CacheKind::Centrality {
                mode: "betweenness".into(),
                limit: None,
            },
            OutcomeFormat::Json,
        );
        let k3 = CacheKey::new(
            "ns",
            "db",
            "t",
            CacheKind::Centrality {
                mode: "closeness".into(),
                limit: None,
            },
            OutcomeFormat::Json,
        );

        c.insert(k1.clone(), "res1".into(), 1);
        c.insert(k2.clone(), "res2".into(), 1);
        assert_eq!(c.len(), 2);

        // Inserting k3 exceeds capacity (2), so k1 (least recently used) should be evicted.
        c.insert(k3.clone(), "res3".into(), 1);
        assert_eq!(c.len(), 2);
        assert!(c.get(&k1, 1).is_none());
        assert_eq!(c.get(&k2, 1), Some("res2".into()));
        assert_eq!(c.get(&k3, 1), Some("res3".into()));
    }

    #[test]
    fn lru_get_refreshes_recency() {
        let c = ComputeCache::new(2);
        let k1 = CacheKey::new(
            "ns",
            "db",
            "t",
            CacheKind::Centrality {
                mode: "m1".into(),
                limit: None,
            },
            OutcomeFormat::Json,
        );
        let k2 = CacheKey::new(
            "ns",
            "db",
            "t",
            CacheKind::Centrality {
                mode: "m2".into(),
                limit: None,
            },
            OutcomeFormat::Json,
        );
        let k3 = CacheKey::new(
            "ns",
            "db",
            "t",
            CacheKind::Centrality {
                mode: "m3".into(),
                limit: None,
            },
            OutcomeFormat::Json,
        );

        c.insert(k1.clone(), "res1".into(), 1);
        c.insert(k2.clone(), "res2".into(), 1);

        // Access k1 so k1 becomes MRU and k2 becomes LRU.
        assert_eq!(c.get(&k1, 1), Some("res1".into()));

        // Insert k3 -> should evict k2 now, leaving k1 and k3.
        c.insert(k3.clone(), "res3".into(), 1);
        assert_eq!(c.len(), 2);
        assert_eq!(c.get(&k1, 1), Some("res1".into()));
        assert!(c.get(&k2, 1).is_none());
        assert_eq!(c.get(&k3, 1), Some("res3".into()));
    }

    #[test]
    fn lru_evicts_when_byte_limit_exceeded() {
        // Capacity 10 entries, but max 500 bytes. Each entry takes payload.len() + 128 bytes (~328 bytes).
        let c = ComputeCache::with_bounds(10, 500);
        let k1 = CacheKey::new(
            "ns",
            "db",
            "t",
            CacheKind::Centrality {
                mode: "m1".into(),
                limit: None,
            },
            OutcomeFormat::Json,
        );
        let k2 = CacheKey::new(
            "ns",
            "db",
            "t",
            CacheKind::Centrality {
                mode: "m2".into(),
                limit: None,
            },
            OutcomeFormat::Json,
        );

        c.insert(k1.clone(), "x".repeat(200), 1);
        assert_eq!(c.len(), 1);

        // Inserting k2 (another ~328 bytes) pushes current_bytes over 500 bytes -> k1 is evicted.
        c.insert(k2.clone(), "y".repeat(200), 1);
        assert_eq!(c.len(), 1);
        assert!(c.get(&k1, 1).is_none());
        assert!(c.get(&k2, 1).is_some());
    }

    #[test]
    fn test_parse_bytes_str() {
        assert_eq!(parse_bytes_str("1G"), Some(1024 * 1024 * 1024));
        assert_eq!(parse_bytes_str("512M"), Some(512 * 1024 * 1024));
        assert_eq!(parse_bytes_str("100K"), Some(100 * 1024));
        assert_eq!(parse_bytes_str("1048576"), Some(1048576));
    }
}
