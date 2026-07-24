use super::*;

// ---------------------------------------------------------------------------
// GAR upload session auth
// ---------------------------------------------------------------------------

pub(super) fn rewrite_location_with_upload_session_auth(
    location: &str,
    upload_session_repo: Option<&str>,
    signing_key: &[u8],
) -> Result<String, Response> {
    let mut url = match Url::parse(location) {
        Ok(url) => url,
        Err(_) => return Ok(location.to_string()),
    };

    // Sign URLs the proxy needs to keep authenticating itself: both
    // GAR's `/artifacts-uploads/...` (separate handler at
    // `proxy_upload_session`) and OCI's `/v2/{repo}/blobs/uploads/{id}`
    // (handled inline in `proxy_push` via the signed-URL bypass).
    // Anything else passes through unchanged.
    if !url.path().starts_with("/artifacts-uploads/") && !is_oci_upload_session_path(url.path()) {
        return Ok(location.to_string());
    }

    let repo_name = upload_session_repo.ok_or_else(|| {
        oci_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_ERROR",
            "Registry upload session authorization context is missing",
        )
    })?;

    if signing_key.is_empty() {
        return Err(oci_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_ERROR",
            "Registry upload session signing key is not configured",
        ));
    }

    let expires_at = chrono::Utc::now().timestamp() + UPLOAD_SESSION_TTL_SECONDS;
    let signature = sign_upload_session(signing_key, url.path(), repo_name, expires_at);

    url.query_pairs_mut()
        .append_pair(UPLOAD_SESSION_VERSION_PARAM, UPLOAD_SESSION_VERSION)
        .append_pair(UPLOAD_SESSION_REPO_PARAM, repo_name)
        .append_pair(UPLOAD_SESSION_EXPIRES_PARAM, &expires_at.to_string())
        .append_pair(UPLOAD_SESSION_SIGNATURE_PARAM, &signature);

    Ok(url.to_string())
}

pub(super) fn verify_upload_session_auth(
    signing_key: &[u8],
    upload_path: &str,
    query: &HashMap<String, String>,
) -> Result<String, Response> {
    if signing_key.is_empty() {
        return Err(oci_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_ERROR",
            "Registry upload session signing key is not configured",
        ));
    }

    let Some(version) = query.get(UPLOAD_SESSION_VERSION_PARAM) else {
        return Err(invalid_upload_session_auth());
    };
    if version != UPLOAD_SESSION_VERSION {
        return Err(invalid_upload_session_auth());
    }

    let repo_name = query
        .get(UPLOAD_SESSION_REPO_PARAM)
        .filter(|value| !value.is_empty())
        .ok_or_else(invalid_upload_session_auth)?;
    let expires_at = query
        .get(UPLOAD_SESSION_EXPIRES_PARAM)
        .and_then(|value| value.parse::<i64>().ok())
        .ok_or_else(invalid_upload_session_auth)?;
    let signature = query
        .get(UPLOAD_SESSION_SIGNATURE_PARAM)
        .filter(|value| !value.is_empty())
        .ok_or_else(invalid_upload_session_auth)?;

    if expires_at < chrono::Utc::now().timestamp() {
        return Err(invalid_upload_session_auth());
    }

    if !verify_upload_session_signature(signing_key, upload_path, repo_name, expires_at, signature)
    {
        return Err(invalid_upload_session_auth());
    }

    Ok(repo_name.clone())
}

fn invalid_upload_session_auth() -> Response {
    oci_error(
        StatusCode::FORBIDDEN,
        "DENIED",
        "Invalid registry upload session authorization.",
    )
}

pub(super) fn strip_upload_session_auth_params(
    query: &HashMap<String, String>,
) -> HashMap<String, String> {
    query
        .iter()
        .filter(|(key, _)| !is_upload_session_auth_param(key))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn is_upload_session_auth_param(key: &str) -> bool {
    matches!(
        key,
        UPLOAD_SESSION_VERSION_PARAM
            | UPLOAD_SESSION_REPO_PARAM
            | UPLOAD_SESSION_EXPIRES_PARAM
            | UPLOAD_SESSION_SIGNATURE_PARAM
    )
}

pub(super) fn sign_upload_session(
    signing_key: &[u8],
    upload_path: &str,
    repo_name: &str,
    expires_at: i64,
) -> String {
    let upload_signing_key = derive_upload_session_signing_key(signing_key);
    let mut mac = HmacSha256::new_from_slice(&upload_signing_key).expect("HMAC accepts any key");
    mac.update(upload_session_payload(upload_path, repo_name, expires_at).as_bytes());
    URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
}

fn verify_upload_session_signature(
    signing_key: &[u8],
    upload_path: &str,
    repo_name: &str,
    expires_at: i64,
    signature: &str,
) -> bool {
    let Ok(signature) = URL_SAFE_NO_PAD.decode(signature) else {
        return false;
    };

    let upload_signing_key = derive_upload_session_signing_key(signing_key);
    let mut mac = HmacSha256::new_from_slice(&upload_signing_key).expect("HMAC accepts any key");
    mac.update(upload_session_payload(upload_path, repo_name, expires_at).as_bytes());
    mac.verify_slice(&signature).is_ok()
}

fn derive_upload_session_signing_key(signing_key: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(signing_key).expect("HMAC accepts any key");
    mac.update(UPLOAD_SESSION_SIGNING_CONTEXT);
    mac.finalize().into_bytes().to_vec()
}

fn upload_session_payload(upload_path: &str, repo_name: &str, expires_at: i64) -> String {
    format!(
        "{}\n{}\n{}\n{}",
        UPLOAD_SESSION_VERSION, upload_path, repo_name, expires_at
    )
}
