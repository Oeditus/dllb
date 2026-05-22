//! `StorageWriter` -- a joerl GenServer that serializes write access to redb.
//!
//! Reads bypass the actor entirely via direct `read_txn` on the shared
//! `Arc<Database>` handle. Only writes go through the actor mailbox.

use async_trait::async_trait;

use joerl::gen_server::{CallResponse, GenServer, GenServerContext};

use crate::backend::RedbBackend;
use crate::kv::KvStore;

// ---------------------------------------------------------------------------
// Message types
// ---------------------------------------------------------------------------

/// Synchronous requests (call) -- the caller blocks until a reply arrives.
#[derive(Debug)]
pub enum StorageCall {
    Get { key: Vec<u8> },
    Scan { start: Vec<u8>, end: Vec<u8> },
    PrefixScan { prefix: Vec<u8> },
    Contains { key: Vec<u8> },
}

/// Fire-and-forget requests (cast) -- no reply.
#[derive(Debug)]
pub enum StorageCast {
    Put { key: Vec<u8>, value: Vec<u8> },
    Delete { key: Vec<u8> },
    PutBatch { ops: Vec<(Vec<u8>, Vec<u8>)> },
    DeleteBatch { keys: Vec<Vec<u8>> },
}

/// Replies to `StorageCall` messages.
#[derive(Debug)]
pub enum StorageReply {
    Value(Option<Vec<u8>>),
    Entries(Vec<(Vec<u8>, Vec<u8>)>),
    Bool(bool),
}

// ---------------------------------------------------------------------------
// GenServer implementation
// ---------------------------------------------------------------------------

/// The GenServer that owns write access to the storage backend.
pub struct StorageWriter;

#[async_trait]
impl GenServer for StorageWriter {
    type State = RedbBackend;
    type Call = StorageCall;
    type Cast = StorageCast;
    type CallReply = StorageReply;

    async fn init(&mut self, _ctx: &mut GenServerContext<'_, Self>) -> Self::State {
        // State is injected externally when spawning; this is a placeholder.
        // In practice, the caller constructs RedbBackend and passes it as
        // initial state via the spawn helper.
        unreachable!("StorageWriter must be spawned with pre-built RedbBackend state")
    }

    async fn handle_call(
        &mut self,
        call: Self::Call,
        state: &mut Self::State,
        _ctx: &mut GenServerContext<'_, Self>,
    ) -> CallResponse<Self::CallReply> {
        let reply = match call {
            StorageCall::Get { key } => {
                let result = state.get(&key).unwrap_or(None);
                StorageReply::Value(result)
            }
            StorageCall::Scan { start, end } => {
                let result = state.scan(&start, &end).unwrap_or_default();
                StorageReply::Entries(result)
            }
            StorageCall::PrefixScan { prefix } => {
                let result = state.prefix_scan(&prefix).unwrap_or_default();
                StorageReply::Entries(result)
            }
            StorageCall::Contains { key } => {
                let result = state.contains(&key).unwrap_or(false);
                StorageReply::Bool(result)
            }
        };
        CallResponse::Reply(reply)
    }

    async fn handle_cast(
        &mut self,
        cast: Self::Cast,
        state: &mut Self::State,
        _ctx: &mut GenServerContext<'_, Self>,
    ) {
        match cast {
            StorageCast::Put { key, value } => {
                if let Err(e) = state.put(&key, &value) {
                    tracing::error!("StorageWriter put failed: {e}");
                }
            }
            StorageCast::Delete { key } => {
                if let Err(e) = state.delete(&key) {
                    tracing::error!("StorageWriter delete failed: {e}");
                }
            }
            StorageCast::PutBatch { ops } => {
                let refs: Vec<(&[u8], &[u8])> = ops
                    .iter()
                    .map(|(k, v)| (k.as_slice(), v.as_slice()))
                    .collect();
                if let Err(e) = state.put_batch(&refs) {
                    tracing::error!("StorageWriter put_batch failed: {e}");
                }
            }
            StorageCast::DeleteBatch { keys } => {
                let refs: Vec<&[u8]> = keys.iter().map(|k| k.as_slice()).collect();
                if let Err(e) = state.delete_batch(&refs) {
                    tracing::error!("StorageWriter delete_batch failed: {e}");
                }
            }
        }
    }
}
