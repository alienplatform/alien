//! Persistent storage for sandboxes: a directory whose contents belong to a caller-chosen key.
//!
//! This crate is the cloud-neutral part. It defines how a key's tree is laid out in object
//! storage, how a commit is built and published, how a tree is restored and how unreferenced
//! objects are collected. A transport (the agent pushing with presigned rights, or a binding
//! pulling over the agent's inbound channel) only has to move objects; the layout, the one-writer
//! rule and the integrity checks are here.
//!
//! # Layout
//!
//! Under `<key>/` in the store setup creates for the sandbox's persistent storage:
//!
//! - `packs/<seq>-<sha256>.tar`: a tar of 64 KiB content-addressed chunks, each member named by
//!   the SHA-256 of its bytes.
//! - `manifests/<seq>-<sha256>.json`: the whole tree at commit `seq` (path, type, mode, owner,
//!   mtime, size, chunks, hard links, symlink targets) and where every chunk it uses lives.
//! - `HEAD`: the current manifest's name (and so its hash), moved only with a conditional put.
//!
//! Packs and manifests are named by their sequence number and the SHA-256 of their bytes, and
//! written with `If-None-Match: *` where the transport allows it, so a superseded writer racing
//! the live one can never change what a name HEAD reaches means.
//!
//! # One writer per key
//!
//! A writer remembers HEAD's version (`ETag`, or the GCS generation) and moves HEAD only if it is unchanged. A new
//! sandbox for a key takes over by rewriting HEAD with its own writer id, after which every
//! commit of the previous sandbox is refused and its state becomes `stopped`. Nothing waits on
//! a lock.

#[cfg(feature = "azure-blob")]
mod azure_blob;
mod commit;
mod error;
mod gc;
mod layout;
mod manifest;
mod pack;
mod read;
mod rights;
mod store;

#[cfg(feature = "azure-blob")]
pub use azure_blob::AzureBlobStore;
pub use commit::{CommitOutcome, Committer, FileSource, TreeEntry, TreeEntryKind};
pub use error::{ErrorData, Result};
pub use gc::{collect_garbage, GcReport};
pub use layout::{
    key_root, validate_key, validate_tree_path, Head, ObjectName, HEAD_OBJECT, MANIFESTS_DIR,
    PACKS_DIR,
};
pub use manifest::{ChunkLocation, Entry, EntryKind, Manifest, MANIFEST_VERSION};
pub use pack::{chunk_hash, PackBuilder, CHUNK_SIZE};
pub use read::{read_file, read_head, read_manifest, restore, takeover, Takeover, TreeSink};
pub use rights::{S3PostPolicy, StorageGrant};
pub use store::{
    create, MemoryStore, ObjectStore, ObjectVersion, Precondition, PutOutcome, StoredObject,
};
