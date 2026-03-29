use sha2::{Digest, Sha256};

/// Generate a deployment ID with "dp_" prefix.
pub fn deployment_id() -> String {
    format!("dp_{}", nanoid::nanoid!())
}

/// Generate a raw token value with the given prefix.
/// Returns (raw_token, key_prefix, key_hash).
pub fn generate_token(prefix: &str) -> (String, String, String) {
    use rand::Rng;
    let mut rng = rand::rng();
    let random_part: String = (0..40)
        .map(|_| {
            let idx: usize = rng.random_range(0..16);
            "0123456789abcdef".chars().nth(idx).unwrap()
        })
        .collect();

    let raw_token = format!("{}{}", prefix, random_part);
    let key_prefix = raw_token[..12.min(raw_token.len())].to_string();
    let key_hash = sha256_hash(&raw_token);

    (raw_token, key_prefix, key_hash)
}

/// Compute SHA-256 hash of a token string.
pub fn sha256_hash(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}
