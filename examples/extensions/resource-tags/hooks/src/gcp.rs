use alien_extension_sdk::prelude::*;

use crate::TAGS;

/// Cloud SQL keeps labels under settings.userLabels.
pub struct CloudSqlLabels;

impl GcpInterceptor for CloudSqlLabels {
    fn matches(&self, call: &GcpCall) -> bool {
        call.service() == "cloudsql" && call.matches_route("POST", "projects/*/instances")
    }

    fn modify_request(&self, request: &mut GcpRequest) -> Result<()> {
        let labels = &mut request.body_mut()["settings"]["userLabels"];
        for (key, value) in TAGS {
            labels[*key] = json!(value);
        }
        Ok(())
    }
}
