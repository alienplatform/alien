use crate::error::{ErrorData, Result};
use alien_error::AlienError;
use std::collections::HashMap;
use url::Url;

/// Parse a public URL assignment in `<resource-id>=<absolute-url>` form.
pub fn parse_public_url_assignment(value: &str) -> Result<(String, String)> {
    let (resource_id, public_url) = value.split_once('=').ok_or_else(|| {
        AlienError::new(ErrorData::PublicUrlInvalid {
            resource_id: "<missing>".to_string(),
            reason: "expected <resource-id>=<absolute-url>".to_string(),
        })
    })?;
    let resource_id = resource_id.trim();
    let public_url = public_url.trim();
    validate_public_url(resource_id, public_url)?;
    Ok((resource_id.to_string(), public_url.to_string()))
}

/// Validate a map of externally supplied public URLs keyed by resource ID.
pub fn validate_public_urls(public_urls: &HashMap<String, String>) -> Result<()> {
    for (resource_id, public_url) in public_urls {
        validate_public_url(resource_id, public_url)?;
    }
    Ok(())
}

/// Validate one externally supplied public URL.
pub fn validate_public_url(resource_id: &str, public_url: &str) -> Result<()> {
    if resource_id.trim().is_empty() {
        return Err(AlienError::new(ErrorData::PublicUrlInvalid {
            resource_id: "<empty>".to_string(),
            reason: "resource ID is required".to_string(),
        }));
    }
    if resource_id.trim() != resource_id {
        return Err(AlienError::new(ErrorData::PublicUrlInvalid {
            resource_id: resource_id.to_string(),
            reason: "resource ID must not contain leading or trailing whitespace".to_string(),
        }));
    }
    if public_url.trim().is_empty() {
        return Err(AlienError::new(ErrorData::PublicUrlInvalid {
            resource_id: resource_id.to_string(),
            reason: "URL is required".to_string(),
        }));
    }

    let parsed = Url::parse(public_url).map_err(|err| {
        AlienError::new(ErrorData::PublicUrlInvalid {
            resource_id: resource_id.to_string(),
            reason: format!("URL must be absolute: {err}"),
        })
    })?;

    match parsed.scheme() {
        "http" | "https" => {}
        scheme => {
            return Err(AlienError::new(ErrorData::PublicUrlInvalid {
                resource_id: resource_id.to_string(),
                reason: format!("URL scheme must be http or https, got '{scheme}'"),
            }));
        }
    }

    let Some(host) = parsed.host_str() else {
        return Err(AlienError::new(ErrorData::PublicUrlInvalid {
            resource_id: resource_id.to_string(),
            reason: "URL must include a host".to_string(),
        }));
    };
    if host.contains('*') {
        return Err(AlienError::new(ErrorData::PublicUrlInvalid {
            resource_id: resource_id.to_string(),
            reason: "URL host must be the base resource hostname, not a wildcard".to_string(),
        }));
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(AlienError::new(ErrorData::PublicUrlInvalid {
            resource_id: resource_id.to_string(),
            reason: "URL must not include query parameters or a fragment".to_string(),
        }));
    }
    if parsed.path() != "/" {
        return Err(AlienError::new(ErrorData::PublicUrlInvalid {
            resource_id: resource_id.to_string(),
            reason: "URL path must be empty or '/'".to_string(),
        }));
    }

    Ok(())
}

/// Return the host part of an already-validated public URL.
pub fn public_url_host(public_url: &str) -> Option<String> {
    Url::parse(public_url)
        .ok()
        .and_then(|url| {
            url.host_str()
                .map(|host| host.trim_end_matches('.').to_string())
        })
        .filter(|host| !host.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_public_url_assignment() {
        let (resource_id, public_url) =
            parse_public_url_assignment("gateway=https://gateway.example.test")
                .expect("assignment should parse");

        assert_eq!(resource_id, "gateway");
        assert_eq!(public_url, "https://gateway.example.test");
    }

    #[test]
    fn rejects_invalid_public_urls() {
        for value in [
            "gateway",
            "=https://gateway.example.test",
            "gateway=",
            "gateway=ftp://gateway.example.test",
            "gateway=https://*.gateway.example.test",
            "gateway=https://gateway.example.test/path",
            "gateway=https://gateway.example.test?x=1",
            "gateway=https://gateway.example.test#frag",
        ] {
            assert!(
                parse_public_url_assignment(value).is_err(),
                "{value} should be invalid"
            );
        }
    }

    #[test]
    fn extracts_public_url_host() {
        assert_eq!(
            public_url_host("https://gateway.example.test:8443"),
            Some("gateway.example.test".to_string())
        );
        assert_eq!(public_url_host("not a url"), None);
    }
}
