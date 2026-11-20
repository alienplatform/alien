use alien_extension_sdk::prelude::*;

use crate::TAGS;

/// Azure Resource Manager takes tags at the top of the resource body.
pub struct AzureTags;

impl AzureInterceptor for AzureTags {
    fn matches(&self, call: &AzureCall) -> bool {
        call.method() == "PUT"
            && matches!(
                (call.provider(), call.resource_type()),
                ("Microsoft.DBforPostgreSQL", "flexibleServers")
                    | ("Microsoft.Storage", "storageAccounts")
            )
    }

    fn modify_request(&self, request: &mut AzureRequest) -> Result<()> {
        let tags = &mut request.body_mut()["tags"];
        for (key, value) in TAGS {
            tags[*key] = json!(value);
        }
        Ok(())
    }
}
