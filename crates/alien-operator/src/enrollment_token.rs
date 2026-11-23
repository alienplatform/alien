//! The single-string enrollment token an installer hands to the operator:
//! `ax_join_v1_<base64url JSON {u: managerUrl, c: code}>`.

use alien_error::AlienError;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::Deserialize;

use crate::error::{ErrorData, Result};

const PREFIX: &str = "ax_join_v1_";

/// What the token carries, fed to the existing enrollment request.
#[derive(PartialEq, Eq)]
pub(crate) struct EnrollmentToken {
    pub manager_url: String,
    pub code: String,
}

// No `Debug` in a build that ships: the code is a credential. Tests need one to unwrap errors;
// theirs hides it.
#[cfg(test)]
impl std::fmt::Debug for EnrollmentToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnrollmentToken")
            .field("manager_url", &self.manager_url)
            .field("code", &"<redacted>")
            .finish()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    u: String,
    c: String,
}

fn invalid(message: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ConfigurationError {
        message: format!("Invalid enrollment token: {message}"),
    })
}

/// Why a manager URL cannot be used, if it cannot: it must be an HTTPS address
/// (or a loopback address for local development) with no user name, password,
/// query or fragment.
pub(crate) fn manager_url_problem(manager_url: &str) -> Option<&'static str> {
    let Ok(url) = reqwest::Url::parse(manager_url) else {
        return Some("the manager URL is not valid");
    };
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Some("the manager URL must not contain a user name, password, query or fragment");
    }
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if !(url.scheme() == "https" || (url.scheme() == "http" && loopback)) {
        return Some("the manager URL must use HTTPS");
    }
    None
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
        let payload: Payload = serde_json::from_slice(&bytes).map_err(|_| {
            invalid("its content is not readable or has fields this operator does not know (a token issued in an older format must be issued again)")
        })?;
        if payload.u.is_empty() || payload.c.is_empty() {
            return Err(invalid("it is missing the manager or the code"));
        }
        // The code is sent to this URL, so it must be HTTPS (or a loopback
        // address for local development), as for any other enrollment.
        if let Some(problem) = manager_url_problem(&payload.u) {
            return Err(invalid(problem));
        }
        Ok(Self {
            manager_url: payload.u,
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
    fn a_token_yields_the_manager_and_code() {
        let parsed = EnrollmentToken::parse(&token(
            r#"{"u":"https://manager.example","c":"ax_enroll_v1_ab"}"#,
        ))
        .unwrap();
        assert_eq!(parsed.manager_url, "https://manager.example");
        assert_eq!(parsed.code, "ax_enroll_v1_ab");
    }

    #[test]
    fn a_token_in_the_older_format_with_an_audience_is_refused_with_a_clear_message() {
        let error = EnrollmentToken::parse(&token(
            r#"{"u":"https://m.example","a":"aud","c":"ax_enroll_v1_ab"}"#,
        ))
        .unwrap_err()
        .to_string();
        assert!(error.contains("issued again"), "{error}");
    }

    #[test]
    fn plain_http_to_a_remote_manager_is_refused() {
        let error = EnrollmentToken::parse(&token(r#"{"u":"http://manager.example","c":"c"}"#))
            .unwrap_err();
        assert!(error.to_string().contains("HTTPS"));
        assert!(EnrollmentToken::parse(&token(r#"{"u":"http://localhost:8080","c":"c"}"#)).is_ok());
    }

    #[test]
    fn unknown_fields_and_urls_with_credentials_or_queries_are_refused() {
        for (json, expected) in [
            (
                r#"{"u":"https://m.example","c":"c","x":1}"#,
                "does not know",
            ),
            (r#"{"u":"https://user:pw@m.example","c":"c"}"#, "user name"),
            (r#"{"u":"https://m.example/?next=1","c":"c"}"#, "query"),
            (r#"{"u":"https://m.example/#f","c":"c"}"#, "fragment"),
        ] {
            let error = EnrollmentToken::parse(&token(json))
                .unwrap_err()
                .to_string();
            assert!(error.contains(expected), "{json}: {error}");
        }
    }

    #[test]
    fn malformed_tokens_are_refused_without_echoing_them() {
        for bad in [
            "",
            "ax_enroll_v1_abc",
            "ax_join_v1_!!",
            &token("{}"),
            &token(r#"{"u":"https://m","c":""}"#),
        ] {
            let error = EnrollmentToken::parse(bad).unwrap_err().to_string();
            assert!(error.contains("Invalid enrollment token"));
            assert!(bad.is_empty() || !error.contains(bad));
        }
    }
}
