//! The single-string enrollment token an installer hands to the operator:
//! `ax_join_v1_<base64url JSON {u: managerUrl, a: audience, c: code}>`.

use alien_error::AlienError;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::Deserialize;

use crate::error::{ErrorData, Result};

const PREFIX: &str = "ax_join_v1_";

/// What the token carries, fed to the existing enrollment request.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct EnrollmentToken {
    pub manager_url: String,
    pub audience: String,
    pub code: String,
}

#[derive(Deserialize)]
struct Payload {
    u: String,
    a: String,
    c: String,
}

fn invalid(message: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ConfigurationError {
        message: format!("Invalid enrollment token: {message}"),
    })
}

impl EnrollmentToken {
    pub(crate) fn parse(token: &str) -> Result<Self> {
        let body = token
            .trim()
            .strip_prefix(PREFIX)
            .ok_or_else(|| invalid("it does not start with `ax_join_v1_`"))?;
        let bytes = URL_SAFE_NO_PAD
            .decode(body.trim_end_matches('='))
            .map_err(|_| invalid("it is not valid base64url"))?;
        let payload: Payload =
            serde_json::from_slice(&bytes).map_err(|_| invalid("its content is not readable"))?;
        if payload.u.is_empty() || payload.a.is_empty() || payload.c.is_empty() {
            return Err(invalid("it is missing the manager, audience or code"));
        }
        // The code is sent to this URL, so it must be HTTPS (or a loopback
        // address for local development), as for any other enrollment.
        let url =
            reqwest::Url::parse(&payload.u).map_err(|_| invalid("the manager URL is not valid"))?;
        let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        if !(url.scheme() == "https" || (url.scheme() == "http" && loopback)) {
            return Err(invalid("the manager URL must use HTTPS"));
        }
        Ok(Self {
            manager_url: payload.u,
            audience: payload.a,
            code: payload.c,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(json: &str) -> String {
        format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(json))
    }

    #[test]
    fn a_token_yields_the_manager_audience_and_code() {
        let parsed = EnrollmentToken::parse(&token(
            r#"{"u":"https://manager.example","a":"aud","c":"ax_enroll_v1_ab"}"#,
        ))
        .unwrap();
        assert_eq!(parsed.manager_url, "https://manager.example");
        assert_eq!(parsed.audience, "aud");
        assert_eq!(parsed.code, "ax_enroll_v1_ab");
    }

    #[test]
    fn plain_http_to_a_remote_manager_is_refused() {
        let error =
            EnrollmentToken::parse(&token(r#"{"u":"http://manager.example","a":"a","c":"c"}"#))
                .unwrap_err();
        assert!(error.to_string().contains("HTTPS"));
        assert!(
            EnrollmentToken::parse(&token(r#"{"u":"http://localhost:8080","a":"a","c":"c"}"#))
                .is_ok()
        );
    }

    #[test]
    fn malformed_tokens_are_refused_without_echoing_them() {
        for bad in [
            "",
            "ax_enroll_v1_abc",
            "ax_join_v1_!!",
            &token("{}"),
            &token(r#"{"u":"https://m","a":"","c":"c"}"#),
        ] {
            let error = EnrollmentToken::parse(bad).unwrap_err().to_string();
            assert!(error.contains("Invalid enrollment token"));
            assert!(bad.is_empty() || !error.contains(bad));
        }
    }
}
