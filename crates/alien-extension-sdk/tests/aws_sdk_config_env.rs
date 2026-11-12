//! `ctx.aws_sdk_config()` takes nothing from the controller's environment: its own process, since
//! it sets the variables an AWS SDK would read.

mod common;

// `#[controller]` in `common` names the prelude's modules from the crate root.
#[allow(unused_imports)]
use alien_extension_sdk::controller_prelude::*;
use common::{aws_credentials, host, request, stepped, FAR_DEADLINE};
use serde_json::{json, Value};

#[tokio::test(flavor = "multi_thread")]
async fn the_environments_aws_settings_do_not_reach_the_config() {
    // SAFETY: set before the runtime's step runs, and nothing else in this process reads them.
    unsafe {
        std::env::set_var("AWS_ACCESS_KEY_ID", "AKIAENVIRONMENT");
        std::env::set_var("AWS_SECRET_ACCESS_KEY", "env-secret");
        std::env::set_var("AWS_REGION", "ap-south-1");
        std::env::set_var("AWS_PROFILE", "no-such-profile");
        std::env::set_var("AWS_ENDPOINT_URL", "http://127.0.0.1:9/elsewhere");
        std::env::set_var("AWS_USE_FIPS_ENDPOINT", "true");
    }
    let state = stepped(
        host(None)
            .handle(request(aws_credentials(), json!({}), FAR_DEADLINE))
            .await,
    );
    assert_eq!(state["accessKeyId"], "ASIASTEPKEY");
    assert_eq!(state["region"], "eu-west-3");
    assert_eq!(
        state["endpoint"],
        Value::Null,
        "AWS_ENDPOINT_URL was ignored"
    );
}
