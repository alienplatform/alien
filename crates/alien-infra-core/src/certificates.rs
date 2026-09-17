/// Splits a PEM certificate chain into leaf certificate and intermediate chain.
///
/// Returns (leaf_cert, Some(intermediate_chain)) or (leaf_cert, None) if no intermediates.
pub fn split_certificate_chain(pem_chain: &str) -> (String, Option<String>) {
    let certs = pem::parse_many(pem_chain).expect("Failed to parse PEM certificate chain");

    if certs.is_empty() {
        return (pem_chain.to_string(), None);
    }

    let leaf = pem::encode(&certs[0]);
    let chain = if certs.len() > 1 {
        Some(
            certs[1..]
                .iter()
                .map(|c| pem::encode(c))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    } else {
        None
    };

    (leaf, chain)
}
