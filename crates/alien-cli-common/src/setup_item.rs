//! Customer infrastructure a setup link can include.

use clap::ValueEnum;

/// A setup item, as people type it on the command line.
///
/// The names match `alien onboard --setup-items` and the docs. Two of them differ
/// from the API's names (`application` is `deployment`, `storage` is `bucket`);
/// the API names are accepted as aliases so either spelling works.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum SetupItem {
    /// The application itself.
    #[value(alias = "deployment")]
    Application,
    /// AI models in the customer's cloud.
    Models,
    /// Customer-managed encryption keys.
    Keys,
    /// A storage bucket in the customer's cloud.
    #[value(alias = "bucket")]
    Storage,
    /// A container registry in the customer's cloud.
    Registry,
    /// A remote sandbox.
    Sandbox,
}

impl SetupItem {
    /// The name shown to people and accepted by `--setup-item`.
    pub fn cli_name(self) -> &'static str {
        match self {
            Self::Application => "application",
            Self::Models => "models",
            Self::Keys => "keys",
            Self::Storage => "storage",
            Self::Registry => "registry",
            Self::Sandbox => "sandbox",
        }
    }

    /// The name the API uses for this item (`setupItem` on the wire).
    pub fn api_name(self) -> &'static str {
        match self {
            Self::Application => "deployment",
            Self::Models => "models",
            Self::Keys => "keys",
            Self::Storage => "bucket",
            Self::Registry => "registry",
            Self::Sandbox => "sandbox",
        }
    }

    /// Parse the API's name for an item. `None` for a name this CLI does not know.
    pub fn from_api_name(name: &str) -> Option<Self> {
        Self::value_variants()
            .iter()
            .copied()
            .find(|item| item.api_name() == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Args {
        #[arg(long = "setup-item")]
        setup_item: SetupItem,
    }

    fn parse(value: &str) -> SetupItem {
        Args::try_parse_from(["cli", "--setup-item", value])
            .unwrap_or_else(|error| panic!("--setup-item {value} should parse: {error}"))
            .setup_item
    }

    /// `alien onboard` prints these names, so the deploy CLIs must accept them and send the
    /// API's names. Sending `application` used to fail with "unknown variant".
    #[test]
    fn onboard_names_and_api_names_select_the_same_item() {
        for item in SetupItem::value_variants() {
            assert_eq!(parse(item.cli_name()), *item);
            assert_eq!(parse(item.api_name()), *item);
            assert_eq!(SetupItem::from_api_name(item.api_name()), Some(*item));
        }
        assert_eq!(parse("application").api_name(), "deployment");
        assert_eq!(parse("storage").api_name(), "bucket");
    }

    #[test]
    fn unknown_names_are_rejected_before_any_request() {
        let error = Args::try_parse_from(["cli", "--setup-item", "app"])
            .err()
            .expect("an unknown item must not parse");
        assert!(error.to_string().contains("application"), "{error}");
        assert_eq!(SetupItem::from_api_name("application"), None);
    }
}
