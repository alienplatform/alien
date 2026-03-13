//! Shared network CLI arguments and parsing for alien-cli and alien-project-cli.
//!
//! Provides [`NetworkArgs`] (a clap `Args` struct) and [`parse_network_settings`] to convert
//! CLI flags into [`alien_core::NetworkSettings`].

use alien_core::NetworkSettings;
use clap::Args;

/// Network mode for the `--network` flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkMode {
    Auto,
    UseDefault,
    Create,
    Byo,
}

impl std::str::FromStr for NetworkMode {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "auto" => Ok(Self::Auto),
            "use-default" => Ok(Self::UseDefault),
            "create" => Ok(Self::Create),
            "byo" => Ok(Self::Byo),
            _ => Err(format!(
                "invalid network mode '{}': expected auto, use-default, create, or byo",
                s
            )),
        }
    }
}

impl std::fmt::Display for NetworkMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auto => write!(f, "auto"),
            Self::UseDefault => write!(f, "use-default"),
            Self::Create => write!(f, "create"),
            Self::Byo => write!(f, "byo"),
        }
    }
}

/// CLI arguments for network configuration. Embed in deploy args with `#[command(flatten)]`.
#[derive(Args, Debug, Clone)]
#[command(next_help_heading = "Network options")]
pub struct NetworkArgs {
    /// Network mode: auto, use-default, create, byo.
    ///
    /// auto         — system decides (create VPC if containers need it, skip otherwise)
    /// use-default  — use cloud provider's default VPC (fast, good for dev)
    /// create       — create an isolated VPC (production-grade)
    /// byo          — bring your own VPC/VNet
    #[arg(long = "network", default_value = "auto")]
    pub network_mode: NetworkMode,

    // -- Create mode options --
    /// VPC CIDR block (create mode only, auto-generated if omitted)
    #[arg(long)]
    pub network_cidr: Option<String>,

    /// Number of availability zones (create mode only, default: 2)
    #[arg(long)]
    pub availability_zones: Option<u8>,

    // -- BYO mode options (AWS) --
    /// Existing VPC ID (byo mode, AWS)
    #[arg(long)]
    pub vpc_id: Option<String>,

    /// Comma-separated public subnet IDs (byo mode, AWS)
    #[arg(long, value_delimiter = ',')]
    pub public_subnet_ids: Vec<String>,

    /// Comma-separated private subnet IDs (byo mode, AWS)
    #[arg(long, value_delimiter = ',')]
    pub private_subnet_ids: Vec<String>,

    /// Comma-separated security group IDs (byo mode, AWS, optional)
    #[arg(long, value_delimiter = ',')]
    pub security_group_ids: Vec<String>,

    // -- BYO mode options (GCP) --
    /// Existing VPC network name (byo mode, GCP)
    #[arg(long)]
    pub network_name: Option<String>,

    /// Subnet name (byo mode, GCP)
    #[arg(long)]
    pub subnet_name: Option<String>,

    /// Subnet region (byo mode, GCP)
    #[arg(long)]
    pub network_region: Option<String>,

    // -- BYO mode options (Azure) --
    /// Existing VNet resource ID (byo mode, Azure)
    #[arg(long)]
    pub vnet_resource_id: Option<String>,

    /// Public subnet name (byo mode, Azure)
    #[arg(long)]
    pub public_subnet_name: Option<String>,

    /// Private subnet name (byo mode, Azure)
    #[arg(long)]
    pub private_subnet_name: Option<String>,
}

/// Parse CLI network flags into `Option<NetworkSettings>`.
///
/// Returns `None` for auto mode (system decides), or `Some(settings)` for explicit modes.
/// The `platform` string is used to validate BYO options match the target platform.
pub fn parse_network_settings(
    args: &NetworkArgs,
    platform: &str,
) -> std::result::Result<Option<NetworkSettings>, String> {
    match &args.network_mode {
        NetworkMode::Auto => {
            reject_all_sub_flags(args, "auto")?;
            Ok(None)
        }
        NetworkMode::UseDefault => {
            reject_all_sub_flags(args, "use-default")?;
            Ok(Some(NetworkSettings::UseDefault))
        }
        NetworkMode::Create => {
            reject_byo_flags(args, "create")?;
            Ok(Some(NetworkSettings::Create {
                cidr: args.network_cidr.clone(),
                availability_zones: args.availability_zones.unwrap_or(2),
            }))
        }
        NetworkMode::Byo => {
            reject_create_flags(args, "byo")?;
            parse_byo_settings(args, platform)
        }
    }
}

fn reject_create_flags(args: &NetworkArgs, mode: &str) -> std::result::Result<(), String> {
    if args.network_cidr.is_some() {
        return Err(format!(
            "--network-cidr is not valid with --network {}",
            mode
        ));
    }
    if args.availability_zones.is_some() {
        return Err(format!(
            "--availability-zones is not valid with --network {}",
            mode
        ));
    }
    Ok(())
}

fn reject_byo_flags(args: &NetworkArgs, mode: &str) -> std::result::Result<(), String> {
    let byo_flags = [
        (args.vpc_id.is_some(), "--vpc-id"),
        (!args.public_subnet_ids.is_empty(), "--public-subnet-ids"),
        (!args.private_subnet_ids.is_empty(), "--private-subnet-ids"),
        (!args.security_group_ids.is_empty(), "--security-group-ids"),
        (args.network_name.is_some(), "--network-name"),
        (args.subnet_name.is_some(), "--subnet-name"),
        (args.network_region.is_some(), "--network-region"),
        (args.vnet_resource_id.is_some(), "--vnet-resource-id"),
        (args.public_subnet_name.is_some(), "--public-subnet-name"),
        (args.private_subnet_name.is_some(), "--private-subnet-name"),
    ];
    for (present, flag) in byo_flags {
        if present {
            return Err(format!("{} is not valid with --network {}", flag, mode));
        }
    }
    Ok(())
}

fn reject_all_sub_flags(args: &NetworkArgs, mode: &str) -> std::result::Result<(), String> {
    reject_create_flags(args, mode)?;
    reject_byo_flags(args, mode)?;
    Ok(())
}

fn parse_byo_settings(
    args: &NetworkArgs,
    platform: &str,
) -> std::result::Result<Option<NetworkSettings>, String> {
    match platform {
        "aws" => {
            let vpc_id = args
                .vpc_id
                .clone()
                .ok_or_else(|| "--vpc-id is required for --network byo on AWS".to_string())?;
            if args.public_subnet_ids.is_empty() {
                return Err("--public-subnet-ids is required for --network byo on AWS".to_string());
            }
            if args.private_subnet_ids.is_empty() {
                return Err("--private-subnet-ids is required for --network byo on AWS".to_string());
            }
            Ok(Some(NetworkSettings::ByoVpcAws {
                vpc_id,
                public_subnet_ids: args.public_subnet_ids.clone(),
                private_subnet_ids: args.private_subnet_ids.clone(),
                security_group_ids: args.security_group_ids.clone(),
            }))
        }
        "gcp" => {
            let network_name = args
                .network_name
                .clone()
                .ok_or_else(|| "--network-name is required for --network byo on GCP".to_string())?;
            let subnet_name = args
                .subnet_name
                .clone()
                .ok_or_else(|| "--subnet-name is required for --network byo on GCP".to_string())?;
            let region = args.network_region.clone().ok_or_else(|| {
                "--network-region is required for --network byo on GCP".to_string()
            })?;
            Ok(Some(NetworkSettings::ByoVpcGcp {
                network_name,
                subnet_name,
                region,
            }))
        }
        "azure" => {
            let vnet_resource_id = args.vnet_resource_id.clone().ok_or_else(|| {
                "--vnet-resource-id is required for --network byo on Azure".to_string()
            })?;
            let public_subnet_name = args.public_subnet_name.clone().ok_or_else(|| {
                "--public-subnet-name is required for --network byo on Azure".to_string()
            })?;
            let private_subnet_name = args.private_subnet_name.clone().ok_or_else(|| {
                "--private-subnet-name is required for --network byo on Azure".to_string()
            })?;
            Ok(Some(NetworkSettings::ByoVnetAzure {
                vnet_resource_id,
                public_subnet_name,
                private_subnet_name,
            }))
        }
        _ => Err(format!(
            "--network byo is not supported on platform '{}' (supported: aws, gcp, azure)",
            platform
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_args() -> NetworkArgs {
        NetworkArgs {
            network_mode: NetworkMode::Auto,
            network_cidr: None,
            availability_zones: None,
            vpc_id: None,
            public_subnet_ids: vec![],
            private_subnet_ids: vec![],
            security_group_ids: vec![],
            network_name: None,
            subnet_name: None,
            network_region: None,
            vnet_resource_id: None,
            public_subnet_name: None,
            private_subnet_name: None,
        }
    }

    #[test]
    fn auto_returns_none() {
        let args = default_args();
        let result = parse_network_settings(&args, "aws").expect("should succeed");
        assert!(result.is_none());
    }

    #[test]
    fn use_default_returns_use_default() {
        let mut args = default_args();
        args.network_mode = NetworkMode::UseDefault;
        let result = parse_network_settings(&args, "aws")
            .expect("should succeed")
            .expect("should be Some");
        assert_eq!(result, NetworkSettings::UseDefault);
    }

    #[test]
    fn create_with_defaults() {
        let mut args = default_args();
        args.network_mode = NetworkMode::Create;
        let result = parse_network_settings(&args, "aws")
            .expect("should succeed")
            .expect("should be Some");
        assert_eq!(
            result,
            NetworkSettings::Create {
                cidr: None,
                availability_zones: 2,
            }
        );
    }

    #[test]
    fn create_with_custom_options() {
        let mut args = default_args();
        args.network_mode = NetworkMode::Create;
        args.network_cidr = Some("10.42.0.0/16".to_string());
        args.availability_zones = Some(3);
        let result = parse_network_settings(&args, "aws")
            .expect("should succeed")
            .expect("should be Some");
        assert_eq!(
            result,
            NetworkSettings::Create {
                cidr: Some("10.42.0.0/16".to_string()),
                availability_zones: 3,
            }
        );
    }

    #[test]
    fn byo_aws() {
        let mut args = default_args();
        args.network_mode = NetworkMode::Byo;
        args.vpc_id = Some("vpc-123".to_string());
        args.public_subnet_ids = vec!["subnet-a".to_string()];
        args.private_subnet_ids = vec!["subnet-b".to_string()];
        let result = parse_network_settings(&args, "aws")
            .expect("should succeed")
            .expect("should be Some");
        assert_eq!(
            result,
            NetworkSettings::ByoVpcAws {
                vpc_id: "vpc-123".to_string(),
                public_subnet_ids: vec!["subnet-a".to_string()],
                private_subnet_ids: vec!["subnet-b".to_string()],
                security_group_ids: vec![],
            }
        );
    }

    #[test]
    fn byo_aws_missing_vpc_id() {
        let mut args = default_args();
        args.network_mode = NetworkMode::Byo;
        args.public_subnet_ids = vec!["subnet-a".to_string()];
        args.private_subnet_ids = vec!["subnet-b".to_string()];
        let err = parse_network_settings(&args, "aws").unwrap_err();
        assert!(err.contains("--vpc-id is required"));
    }

    #[test]
    fn auto_rejects_create_flags() {
        let mut args = default_args();
        args.network_cidr = Some("10.0.0.0/16".to_string());
        let err = parse_network_settings(&args, "aws").unwrap_err();
        assert!(err.contains("--network-cidr is not valid with --network auto"));
    }

    #[test]
    fn use_default_rejects_byo_flags() {
        let mut args = default_args();
        args.network_mode = NetworkMode::UseDefault;
        args.vpc_id = Some("vpc-123".to_string());
        let err = parse_network_settings(&args, "aws").unwrap_err();
        assert!(err.contains("--vpc-id is not valid with --network use-default"));
    }

    #[test]
    fn byo_gcp() {
        let mut args = default_args();
        args.network_mode = NetworkMode::Byo;
        args.network_name = Some("my-vpc".to_string());
        args.subnet_name = Some("my-subnet".to_string());
        args.network_region = Some("us-central1".to_string());
        let result = parse_network_settings(&args, "gcp")
            .expect("should succeed")
            .expect("should be Some");
        assert_eq!(
            result,
            NetworkSettings::ByoVpcGcp {
                network_name: "my-vpc".to_string(),
                subnet_name: "my-subnet".to_string(),
                region: "us-central1".to_string(),
            }
        );
    }

    #[test]
    fn byo_azure() {
        let mut args = default_args();
        args.network_mode = NetworkMode::Byo;
        args.vnet_resource_id = Some("/subscriptions/.../vnet".to_string());
        args.public_subnet_name = Some("pub-subnet".to_string());
        args.private_subnet_name = Some("priv-subnet".to_string());
        let result = parse_network_settings(&args, "azure")
            .expect("should succeed")
            .expect("should be Some");
        assert_eq!(
            result,
            NetworkSettings::ByoVnetAzure {
                vnet_resource_id: "/subscriptions/.../vnet".to_string(),
                public_subnet_name: "pub-subnet".to_string(),
                private_subnet_name: "priv-subnet".to_string(),
            }
        );
    }

    #[test]
    fn byo_unsupported_platform() {
        let mut args = default_args();
        args.network_mode = NetworkMode::Byo;
        let err = parse_network_settings(&args, "kubernetes").unwrap_err();
        assert!(err.contains("not supported on platform 'kubernetes'"));
    }
}
