use nanoid::nanoid;
use regex::Regex;

/// Defines different ID types with their configuration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IdType {
    Event,
    Deployment,
    DeploymentGroup,
    Release,
    Command,
    Token,
}

/// Configuration for ID generation
#[derive(Debug, Clone)]
pub struct IdConfig {
    prefix: &'static str,
    lowercase: bool,
    length: usize,
}

impl IdConfig {
    const fn new(prefix: &'static str, lowercase: bool, length: usize) -> Self {
        Self {
            prefix,
            lowercase,
            length,
        }
    }
}

/// Get configuration for a specific ID type
const fn get_id_config(id_type: IdType) -> IdConfig {
    match id_type {
        IdType::Event => IdConfig::new("event", false, 28),
        IdType::Deployment => IdConfig::new("dep", true, 28),
        IdType::DeploymentGroup => IdConfig::new("dg", true, 28),
        IdType::Release => IdConfig::new("rel", false, 28),
        IdType::Command => IdConfig::new("cmd", true, 28),
        IdType::Token => IdConfig::new("tok", true, 28),
    }
}

pub const ALPHABET_LOWERCASE: &[char] = &[
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i',
    'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z',
];

pub const ALPHABET_MIXED_CASE: &[char] = &[
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I',
    'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z', 'a', 'b',
    'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's', 't', 'u',
    'v', 'w', 'x', 'y', 'z',
];

/// Generate a new ID for the specified type
pub fn new_id(id_type: IdType) -> String {
    let config = get_id_config(id_type);
    let alphabet = if config.lowercase {
        ALPHABET_LOWERCASE
    } else {
        ALPHABET_MIXED_CASE
    };

    let length = config.length;

    let id = nanoid!(length, &alphabet);

    format!("{}_{}", config.prefix, id)
}

/// Generate a deterministic ID example for a given type
pub fn generate_id_example(id_type: IdType) -> String {
    let config = get_id_config(id_type);

    // Determine character set based on lowercase flag
    let char_set = if config.lowercase {
        ALPHABET_LOWERCASE
    } else {
        ALPHABET_MIXED_CASE
    };

    // Length of the suffix
    let suffix_length = config.length;

    // Seed generation from the prefix
    let mut seed = 0u32;
    for c in config.prefix.chars() {
        seed = (seed.wrapping_mul(31) + c as u32) % 4_294_967_295;
    }

    // Generate the suffix using the pseudo-random function
    let mut suffix = String::with_capacity(suffix_length);
    for _ in 0..suffix_length {
        // Pseudo-random number generator
        seed = (seed.wrapping_mul(1_664_525) + 1_013_904_223) % 4_294_967_295;
        let random_value = seed as f64 / 4_294_967_295.0;

        let index = (random_value * char_set.len() as f64).floor() as usize;
        suffix.push(char_set[index]);
    }

    // Return the complete ID
    format!("{}_{}", config.prefix, suffix)
}

/// Convert camel case to spaced lowercase
fn convert_camel_to_spaces(input: &str) -> String {
    let re = Regex::new(r"([a-z])([A-Z])").unwrap();
    re.replace_all(input, "$1 $2").to_lowercase()
}

/// Capitalize the first letter of a string
fn capitalize_first_letter(input: &str) -> String {
    let mut chars = input.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

/// Generate a regex pattern for validating IDs of a specific type
pub fn id_regex_pattern(id_type: IdType) -> String {
    let config = get_id_config(id_type);
    let alphabet_regex = if config.lowercase {
        "0-9a-z"
    } else {
        "0-9a-zA-Z"
    };

    format!(
        "^{}_[{}]{{{}}}$",
        config.prefix, alphabet_regex, config.length
    )
}

/// Generate an error message for invalid IDs
pub fn id_error_message(id_type: IdType) -> String {
    let config = get_id_config(id_type);
    let type_name = convert_camel_to_spaces(&format!("{:?}", id_type));
    let alphabet_desc = if config.lowercase {
        "lowercase letters and numbers"
    } else {
        "alphanumeric characters"
    };

    format!(
        "{} ID must start with {}_ and can only contain {}.",
        capitalize_first_letter(&type_name),
        config.prefix,
        alphabet_desc
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deployment_ids_match_platform_schema_shape() {
        let id = new_id(IdType::Deployment);
        let pattern = Regex::new(&id_regex_pattern(IdType::Deployment)).unwrap();
        assert!(pattern.is_match(&id), "unexpected deployment id: {id}");
        assert!(id.starts_with("dep_"));
    }
}
