use crate::error::{ErrorData, Result};
use alien_error::AlienError;
use std::collections::HashMap;
use url::Url;

/// Public endpoint URL overrides keyed by resource ID, then endpoint name.
pub type PublicEndpointUrls = HashMap<String, HashMap<String, String>>;

/// Parse a public endpoint assignment in `<resource-id>.<endpoint-name>=<absolute-url>` form.
pub fn parse_public_endpoint_assignment(value: &str) -> Result<(String, String, String)> {
    let (key, public_url) = value.split_once('=').ok_or_else(|| {
        AlienError::new(ErrorData::PublicUrlInvalid {
            resource_id: "<missing>".to_string(),
            reason: "expected <resource-id>.<endpoint-name>=<absolute-url>".to_string(),
        })
    })?;
    let key = key.trim();
    let public_url = public_url.trim();
    let (resource_id, endpoint_name) = key.split_once('.').ok_or_else(|| {
        AlienError::new(ErrorData::PublicUrlInvalid {
            resource_id: key.to_string(),
            reason: "expected <resource-id>.<endpoint-name> before '='".to_string(),
        })
    })?;
    validate_public_endpoint_url(resource_id, endpoint_name, public_url)?;
    Ok((
        resource_id.to_string(),
        endpoint_name.to_string(),
        public_url.to_string(),
    ))
}

/// Validate endpoint URL overrides keyed by resource ID and endpoint name.
pub fn validate_public_endpoint_urls(public_endpoints: &PublicEndpointUrls) -> Result<()> {
    for (resource_id, endpoints) in public_endpoints {
        if endpoints.is_empty() {
            return Err(AlienError::new(ErrorData::PublicUrlInvalid {
                resource_id: resource_id.to_string(),
                reason: "at least one endpoint URL is required when a resource is present"
                    .to_string(),
            }));
        }
        for (endpoint_name, public_url) in endpoints {
            validate_public_endpoint_url(resource_id, endpoint_name, public_url)?;
        }
    }
    Ok(())
}

/// Validate one externally supplied endpoint URL.
pub fn validate_public_endpoint_url(
    resource_id: &str,
    endpoint_name: &str,
    public_url: &str,
) -> Result<()> {
    validate_key_part("resource ID", resource_id, resource_id)?;
    validate_key_part("endpoint name", resource_id, endpoint_name)?;
    if public_url.trim().is_empty() {
        return Err(AlienError::new(ErrorData::PublicUrlInvalid {
            resource_id: resource_id.to_string(),
            reason: format!("URL is required for endpoint '{endpoint_name}'"),
        }));
    }

    let parsed = Url::parse(public_url).map_err(|err| {
        AlienError::new(ErrorData::PublicUrlInvalid {
            resource_id: resource_id.to_string(),
            reason: format!("endpoint '{endpoint_name}' URL must be absolute: {err}"),
        })
    })?;

    match parsed.scheme() {
        "http" | "https" => {}
        scheme => {
            return Err(AlienError::new(ErrorData::PublicUrlInvalid {
                resource_id: resource_id.to_string(),
                reason: format!(
                    "endpoint '{endpoint_name}' URL scheme must be http or https, got '{scheme}'"
                ),
            }));
        }
    }

    let Some(host) = parsed.host_str() else {
        return Err(AlienError::new(ErrorData::PublicUrlInvalid {
            resource_id: resource_id.to_string(),
            reason: format!("endpoint '{endpoint_name}' URL must include a host"),
        }));
    };
    if host.contains('*') {
        return Err(AlienError::new(ErrorData::PublicUrlInvalid {
            resource_id: resource_id.to_string(),
            reason: format!(
                "endpoint '{endpoint_name}' URL host must be the base hostname, not a wildcard"
            ),
        }));
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(AlienError::new(ErrorData::PublicUrlInvalid {
            resource_id: resource_id.to_string(),
            reason: format!(
                "endpoint '{endpoint_name}' URL must not include query parameters or a fragment"
            ),
        }));
    }
    if parsed.path() != "/" {
        return Err(AlienError::new(ErrorData::PublicUrlInvalid {
            resource_id: resource_id.to_string(),
            reason: format!("endpoint '{endpoint_name}' URL path must be empty or '/'"),
        }));
    }

    Ok(())
}

fn validate_key_part(label: &str, resource_id: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(AlienError::new(ErrorData::PublicUrlInvalid {
            resource_id: resource_id.to_string(),
            reason: format!("{label} is required"),
        }));
    }
    if value.trim() != value {
        return Err(AlienError::new(ErrorData::PublicUrlInvalid {
            resource_id: resource_id.to_string(),
            reason: format!("{label} must not contain leading or trailing whitespace"),
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

/// Return the effective port of an already-validated public URL.
pub fn public_url_port(public_url: &str) -> Option<u16> {
    Url::parse(public_url)
        .ok()
        .and_then(|url| url.port_or_known_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_public_endpoint_assignment() {
        let (resource_id, endpoint_name, public_url) =
            parse_public_endpoint_assignment("gateway.api=https://api.example.test")
                .expect("assignment should parse");

        assert_eq!(resource_id, "gateway");
        assert_eq!(endpoint_name, "api");
        assert_eq!(public_url, "https://api.example.test");
    }

    #[test]
    fn rejects_invalid_public_endpoint_urls() {
        for value in [
            "gateway",
            "gateway=https://gateway.example.test",
            ".api=https://gateway.example.test",
            "gateway.=https://gateway.example.test",
            "gateway.api=",
            "gateway.api=ftp://gateway.example.test",
            "gateway.api=https://*.gateway.example.test",
            "gateway.api=https://gateway.example.test/path",
            "gateway.api=https://gateway.example.test?x=1",
            "gateway.api=https://gateway.example.test#frag",
        ] {
            assert!(
                parse_public_endpoint_assignment(value).is_err(),
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

    #[test]
    fn extracts_effective_public_url_port() {
        assert_eq!(public_url_port("https://gateway.example.test"), Some(443));
        assert_eq!(public_url_port("http://localhost:8080"), Some(8080));
        assert_eq!(public_url_port("not a url"), None);
    }
}
