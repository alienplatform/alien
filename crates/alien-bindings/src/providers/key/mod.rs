use crate::error::{ErrorData, Result};
use alien_error::AlienError;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[cfg(feature = "aws")]
pub mod aws;
#[cfg(feature = "azure")]
pub mod azure;
#[cfg(feature = "gcp")]
pub mod gcp;

const MAGIC: &[u8; 4] = b"AKBF";
const VERSION: u8 = 1;
const HEADER_LENGTH: usize = 39;
const MAX_PLAINTEXT_LENGTH: usize = 128;
const MAX_CONTEXT_LENGTH: usize = 16 * 1024;

pub(crate) fn encode_context(context: Option<&BTreeMap<String, String>>) -> Result<Vec<u8>> {
    let empty_context = BTreeMap::new();
    let context = context.unwrap_or(&empty_context);
    let mut encoded = Vec::new();
    encoded.extend_from_slice(&(context.len() as u32).to_be_bytes());
    for (key, value) in context {
        let key = key.as_bytes();
        let value = value.as_bytes();
        encoded.extend_from_slice(&(key.len() as u32).to_be_bytes());
        encoded.extend_from_slice(key);
        encoded.extend_from_slice(&(value.len() as u32).to_be_bytes());
        encoded.extend_from_slice(value);
        if encoded.len() > MAX_CONTEXT_LENGTH {
            return Err(AlienError::new(ErrorData::KeyInputInvalid {
                reason: "canonical context exceeds 16 KiB".to_string(),
            }));
        }
    }
    Ok(encoded)
}

pub(crate) fn frame(plaintext: &[u8], canonical_context: &[u8]) -> Result<Vec<u8>> {
    if plaintext.len() > MAX_PLAINTEXT_LENGTH {
        return Err(AlienError::new(ErrorData::KeyInputInvalid {
            reason: "plaintext exceeds the portable 128-byte limit".to_string(),
        }));
    }
    let mut frame = Vec::with_capacity(HEADER_LENGTH + plaintext.len());
    frame.extend_from_slice(MAGIC);
    frame.push(VERSION);
    frame.extend_from_slice(&(plaintext.len() as u16).to_be_bytes());
    frame.extend_from_slice(&Sha256::digest(canonical_context));
    frame.extend_from_slice(plaintext);
    Ok(frame)
}

pub(crate) fn unframe(frame: &[u8], canonical_context: &[u8]) -> Result<Vec<u8>> {
    if frame.len() < HEADER_LENGTH || &frame[..4] != MAGIC || frame[4] != VERSION {
        return Err(AlienError::new(ErrorData::KeyCiphertextInvalid {
            reason: "decrypted data is not an Alien Key frame".to_string(),
        }));
    }
    let plaintext_length = u16::from_be_bytes([frame[5], frame[6]]) as usize;
    if plaintext_length > MAX_PLAINTEXT_LENGTH || frame.len() != HEADER_LENGTH + plaintext_length {
        return Err(AlienError::new(ErrorData::KeyCiphertextInvalid {
            reason: "decrypted frame has an invalid plaintext length".to_string(),
        }));
    }
    let expected_context_hash = Sha256::digest(canonical_context);
    if frame[7..39] != expected_context_hash[..] {
        return Err(AlienError::new(ErrorData::KeyCiphertextInvalid {
            reason: "context does not match the encrypted value".to_string(),
        }));
    }
    Ok(frame[HEADER_LENGTH..].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_enforces_limits_and_context() {
        let context = BTreeMap::from([
            ("project".to_string(), "example".to_string()),
            ("purpose".to_string(), "root".to_string()),
        ]);
        let canonical = encode_context(Some(&context)).unwrap();
        let framed = frame(&[7; 128], &canonical).unwrap();
        assert_eq!(unframe(&framed, &canonical).unwrap(), vec![7; 128]);
        assert!(frame(&[0; 129], &canonical).is_err());
        assert!(unframe(&framed, &encode_context(None).unwrap()).is_err());
    }

    #[test]
    fn canonical_context_and_frame_match_the_published_vector() {
        let context = BTreeMap::from([
            ("purpose".to_string(), "root".to_string()),
            ("project".to_string(), "example".to_string()),
        ]);
        let canonical = encode_context(Some(&context)).unwrap();
        assert_eq!(
            hex::encode(&canonical),
            "000000020000000770726f6a656374000000076578616d706c6500000007707572706f736500000004726f6f74"
        );
        assert_eq!(
            hex::encode(frame(b"hello", &canonical).unwrap()),
            "414b4246010005faf89ea5b4228220f52708c3fc60f67ede69c57333979676767bf5715253e4a768656c6c6f"
        );
    }
}
