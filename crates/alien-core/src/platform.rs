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

/// How bindings are delivered to the application
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum BindingsMode {
    /// Load bindings directly from environment variables (standalone processes)
    Direct,
    /// Load bindings via gRPC from alien-runtime
    Grpc,
}

impl BindingsMode {
    /// Returns the string representation of the bindings mode.
    pub fn as_str(&self) -> &'static str {
        match self {
            BindingsMode::Direct => "direct",
            BindingsMode::Grpc => "grpc",
        }
    }
}

impl std::fmt::Display for BindingsMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for BindingsMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "direct" => Ok(BindingsMode::Direct),
            "grpc" => Ok(BindingsMode::Grpc),
            _ => Err(format!(
                "Invalid bindings mode: '{}'. Must be 'direct' or 'grpc'",
                s
            )),
        }
    }
}

impl Default for BindingsMode {
    fn default() -> Self {
        BindingsMode::Direct
    }
}
