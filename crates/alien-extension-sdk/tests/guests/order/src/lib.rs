use alien_extension_sdk::prelude::*;

/// Sets `DeletionProtection` on every RDS cluster create to whether acme's interceptor already
/// added IAM authentication to it, which it did only if acme ran first.
struct SeesAcme;

impl AwsInterceptor for SeesAcme {
    fn matches(&self, call: &AwsCall) -> bool {
        call.service() == "rds" && call.operation() == "CreateDBCluster"
    }

    fn modify_request(&self, request: &mut AwsRequest) -> Result<()> {
        let acme_ran = request.param("EnableIAMDatabaseAuthentication") == Some("true");
        request.set("DeletionProtection", acme_ran);
        Ok(())
    }
}

fn register(ext: &mut Extension) {
    ext.aws_interceptor(SeesAcme);
}

alien_extension_sdk::export!("order", register);
