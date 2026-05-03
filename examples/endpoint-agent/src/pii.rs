/// Simple PII detection patterns
///
/// This is a basic implementation for demo purposes. Production systems should use
/// more sophisticated NLP models or dedicated PII detection libraries.
use serde::{Deserialize, Serialize};

/// PII detection result
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PiiDetection {
    pub has_pii: bool,
    pub patterns_found: Vec<String>,
}

/// Scan text for PII patterns
pub fn scan_text(text: &str) -> PiiDetection {
    let mut patterns_found = Vec::new();

    // Email pattern
    if text.contains('@') && text.contains('.') {
        let email_regex =
            regex::Regex::new(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Z|a-z]{2,}\b").unwrap();
        if email_regex.is_match(text) {
            patterns_found.push("email".to_string());
        }
    }

    // SSN pattern (XXX-XX-XXXX)
    if text.contains('-') {
        let ssn_regex = regex::Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").unwrap();
        if ssn_regex.is_match(text) {
            patterns_found.push("ssn".to_string());
        }
    }

    // Credit card pattern (simple 16-digit check)
    let cc_regex = regex::Regex::new(r"\b\d{4}[\s-]?\d{4}[\s-]?\d{4}[\s-]?\d{4}\b").unwrap();
    if cc_regex.is_match(text) {
        patterns_found.push("credit_card".to_string());
    }

    // Phone number pattern
    let phone_regex =
        regex::Regex::new(r"\b(?:\+?1[-.\s]?)?\(?\d{3}\)?[-.\s]?\d{3}[-.\s]?\d{4}\b").unwrap();
    if phone_regex.is_match(text) {
        patterns_found.push("phone".to_string());
    }

    // Common sensitive keywords
    let sensitive_keywords = [
        "password",
        "secret",
        "api_key",
        "apikey",
        "token",
        "credential",
    ];
    let lower_text = text.to_lowercase();
    for keyword in &sensitive_keywords {
        if lower_text.contains(keyword) {
            patterns_found.push(format!("keyword:{}", keyword));
        }
    }

    PiiDetection {
        has_pii: !patterns_found.is_empty(),
        patterns_found,
    }
}

/// File scan result
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileScanResult {
    pub path: String,
    pub reason: String,
}

/// Scan file content for sensitive data
pub fn scan_file_content(path: &str, content: &str) -> Option<FileScanResult> {
    let pii = scan_text(content);

    if pii.has_pii {
        Some(FileScanResult {
            path: path.to_string(),
            reason: format!("Contains: {}", pii.patterns_found.join(", ")),
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_email_detection() {
        let result = scan_text("Contact me at user@example.com");
        assert!(result.has_pii);
        assert!(result.patterns_found.contains(&"email".to_string()));
    }

    #[test]
    fn test_ssn_detection() {
        let result = scan_text("My SSN is 123-45-6789");
        assert!(result.has_pii);
        assert!(result.patterns_found.contains(&"ssn".to_string()));
    }

    #[test]
    fn test_no_pii() {
        let result = scan_text("Just normal text here");
        assert!(!result.has_pii);
        assert!(result.patterns_found.is_empty());
    }
}
