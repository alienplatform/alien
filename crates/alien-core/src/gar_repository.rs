//! Project repository names shared by GAR provisioning and OCI routing.

/// Encode the project suffix using lowercase RFC 4648 base32 without padding.
/// Keep the original project ID as the image path so routing preserves its case.
/// The encoding is injective; it does not truncate or lowercase project IDs.
pub fn project_repository(prefix: &str, project_id: &str) -> Option<String> {
    let suffix = project_id.strip_prefix("prj_")?;
    if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return None;
    }
    const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut encoded = String::new();
    let mut buffer = 0u32;
    let mut bits = 0;
    for byte in suffix.bytes() {
        buffer = (buffer << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            encoded.push(ALPHABET[((buffer >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        encoded.push(ALPHABET[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    let repository = format!("{prefix}-p{encoded}");
    (repository.len() <= 63
        && repository.starts_with(|c: char| c.is_ascii_lowercase())
        && repository
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'))
    .then_some(repository)
}

/// Return the original project ID only when its path agrees with its repository encoding.
pub fn project_id_for_path<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    let (repository, remainder) = path.split_once('/')?;
    let project_id = remainder.split('/').next()?;
    (project_repository(prefix, project_id)?.as_str() == repository).then_some(project_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_paths_preserve_case_and_refuse_a_different_encoded_owner() {
        for id in ["prj_a", "prj_A", "prj_0123456789abcdefghijklmn"] {
            let repository = project_repository("alien-artifacts", id).expect("valid name");
            assert!(repository.len() <= 63);
            let path = format!("{repository}/{id}/worker");
            assert_eq!(project_id_for_path(&path, "alien-artifacts"), Some(id));
            assert_eq!(
                project_id_for_path(&format!("{repository}/prj_other"), "alien-artifacts"),
                None
            );
        }
        assert_ne!(
            project_repository("artifacts", "prj_a"),
            project_repository("artifacts", "prj_A")
        );
        assert_eq!(project_repository(&"p".repeat(64), "prj_a"), None);
        assert_eq!(project_repository("artifacts", "prj_../x"), None);
    }
}
