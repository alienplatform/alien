use serde::{Deserialize, Serialize};

/// Represents the target cloud platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum Platform {
    Aws,
    Gcp,
    Azure,
    Kubernetes,
    Local,
    Test,
}

impl Platform {
    /// All deployable platforms (excludes Test).
    pub const DEPLOYABLE: &[Platform] = &[
        Platform::Aws,
        Platform::Gcp,
        Platform::Azure,
        Platform::Kubernetes,
        Platform::Local,
    ];

    /// Returns the string representation of the platform.
    pub fn as_str(&self) -> &'static str {
        match self {
            Platform::Aws => "aws",
            Platform::Gcp => "gcp",
            Platform::Azure => "azure",
            Platform::Kubernetes => "kubernetes",
            Platform::Local => "local",
            Platform::Test => "test",
        }
    }
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for Platform {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "aws" => Ok(Platform::Aws),
            "gcp" => Ok(Platform::Gcp),
            "azure" => Ok(Platform::Azure),
            "kubernetes" => Ok(Platform::Kubernetes),
            "local" => Ok(Platform::Local),
            "test" => Ok(Platform::Test),
            _ => Err(format!("'{}' is not a valid platform", s)),
        }
    }
}

