use std::process::ExitCode;

use alien_operations_sdk::run_plugin;
use custom_ops::operations;
use reqwest::Url;

#[tokio::main]
async fn main() -> ExitCode {
    let registry = std::env::var("CUSTOM_OPS_BASE_URL")
        .map_err(|_| "set CUSTOM_OPS_BASE_URL to the application's admin URL".to_string())
        .and_then(|value| Url::parse(&value).map_err(|_| "invalid CUSTOM_OPS_BASE_URL".to_string()))
        .and_then(|url| {
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err(
                    "CUSTOM_OPS_BASE_URL must be an HTTP base URL without a query or fragment"
                        .to_string(),
                );
            }
            Ok(url)
        })
        .and_then(|url| operations(url.as_str()).map_err(|error| error.to_string()));
    match registry {
        Ok(registry) => run_plugin(registry).await,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
