use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

/// Size of a content chunk. A file is cut at fixed offsets, so an append or an in-place edit
/// reuploads only the chunks it touches.
pub const CHUNK_SIZE: usize = 64 * 1024;

/// Tar's header block size; a member's data starts one block after its header.
const TAR_BLOCK: u64 = 512;

/// SHA-256 of a chunk, lowercase hex. It names the chunk everywhere.
pub fn chunk_hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Builds one commit's pack: a tar whose members are new chunks named by their hash.
///
/// A pack is a plain tar so it can be inspected with ordinary tools; readers never parse it,
/// they read a chunk by the offset its manifest records.
pub struct PackBuilder {
    tar: tar::Builder<Vec<u8>>,
    chunks: BTreeMap<String, (u64, u32)>,
}

impl Default for PackBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl PackBuilder {
    /// An empty pack.
    pub fn new() -> Self {
        Self {
            tar: tar::Builder::new(Vec::new()),
            chunks: BTreeMap::new(),
        }
    }

    /// Adds a chunk unless this pack already holds it, and returns its hash and its data's
    /// offset and length in the pack.
    pub fn add(&mut self, bytes: &[u8]) -> (String, u64, u32) {
        let hash = chunk_hash(bytes);
        if let Some((offset, len)) = self.chunks.get(&hash) {
            return (hash, *offset, *len);
        }
        let offset = self.tar.get_ref().len() as u64 + TAR_BLOCK;
        let mut header = tar::Header::new_ustar();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_entry_type(tar::EntryType::Regular);
        // A 64-character hex name fits the ustar name field, so no extension header is written
        // and the data is exactly one block after the header.
        self.tar
            .append_data(&mut header, &hash, bytes)
            .expect("writing to a Vec cannot fail");
        let len = u32::try_from(bytes.len()).expect("a chunk is at most CHUNK_SIZE");
        self.chunks.insert(hash.clone(), (offset, len));
        (hash, offset, len)
    }

    /// Whether no chunk was added.
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// The finished tar.
    pub fn finish(self) -> Vec<u8> {
        self.tar.into_inner().expect("writing to a Vec cannot fail")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_point_at_chunk_data_and_tar_reads_back() {
        let mut pack = PackBuilder::new();
        let a = pack.add(b"hello");
        let b = pack.add(&vec![7u8; CHUNK_SIZE]);
        assert_eq!(pack.add(b"hello"), a, "a repeated chunk is stored once");
        let bytes = pack.finish();
        for (data, (_, offset, len)) in [(b"hello".to_vec(), a.clone()), (vec![7u8; CHUNK_SIZE], b)]
        {
            assert_eq!(
                &bytes[offset as usize..offset as usize + len as usize],
                &data[..]
            );
        }
        let mut archive = tar::Archive::new(&bytes[..]);
        let names: Vec<String> = archive
            .entries()
            .unwrap()
            .map(|e| e.unwrap().path().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 2);
        assert_eq!(names[0], a.0);
    }
}
