use alien_extension_sdk::prelude::*;

/// Turns on IAM auth for every RDS cluster in the CloudFormation template.
pub struct CloudFormation;

impl CloudFormationModifier for CloudFormation {
    fn modify(&self, mut template: CfTemplate) -> Result<CfTemplate> {
        for cluster in template.resources_of_type_mut("AWS::RDS::DBCluster") {
            cluster.set_property("EnableIAMDatabaseAuthentication", true);
        }
        Ok(template)
    }
}

/// The same flag in the Terraform module, on AWS, Google Cloud and Azure.
pub struct Terraform;

impl TerraformModifier for Terraform {
    fn modify(&self, mut module: TfModule) -> Result<TfModule> {
        for cluster in module.resources_of_type_mut("aws_rds_cluster") {
            cluster.set_attribute("iam_database_authentication_enabled", true);
        }
        for instance in module.resources_of_type_mut("google_sql_database_instance") {
            let flag = instance.block_mut("settings")?.add_block("database_flags");
            flag.set_attribute("name", "cloudsql.iam_authentication");
            flag.set_attribute("value", "on");
        }
        // Microsoft Entra authentication needs the tenant, which Alien's server block doesn't
        // carry. The client config is the tenant Terraform applies the module in.
        let tenant_id = expr::traversal([
            "data",
            "azurerm_client_config",
            "postgres_iam_auth",
            "tenant_id",
        ]);
        let mut azure = false;
        for server in module.resources_of_type_mut("azurerm_postgresql_flexible_server") {
            server
                .add_block("authentication")
                .set_attribute("active_directory_auth_enabled", true)
                .set_attribute("password_auth_enabled", true)
                .set_attribute("tenant_id", tenant_id.clone());
            azure = true;
        }
        if azure {
            module.add_data_source("azurerm_client_config", "postgres_iam_auth")?;
        }
        Ok(module)
    }
}

pub struct Rds;

impl AwsInterceptor for Rds {
    /// The calls this interceptor sees.
    fn matches(&self, call: &AwsCall) -> bool {
        call.service() == "rds" && matches!(call.operation(), "CreateDBCluster" | "ModifyDBCluster")
    }

    /// Runs before Alien signs the request.
    fn modify_request(&self, request: &mut AwsRequest) -> Result<()> {
        request.set("EnableIAMDatabaseAuthentication", true);
        Ok(())
    }

    /// Runs when the response arrives, before Alien reads it. Optional.
    fn modify_response(&self, _response: &mut AwsResponse) -> Result<()> {
        Ok(())
    }
}

pub struct CloudSql;

impl GcpInterceptor for CloudSql {
    fn matches(&self, call: &GcpCall) -> bool {
        call.service() == "cloudsql" && call.matches_route("POST", "projects/*/instances")
    }

    fn modify_request(&self, request: &mut GcpRequest) -> Result<()> {
        let flags = &mut request.body_mut()["settings"]["databaseFlags"];
        let mut list = flags.as_array().cloned().unwrap_or_default();
        list.push(json!({ "name": "cloudsql.iam_authentication", "value": "on" }));
        *flags = Value::Array(list);
        Ok(())
    }
}

pub struct FlexibleServer;

impl AzureInterceptor for FlexibleServer {
    fn matches(&self, call: &AzureCall) -> bool {
        call.provider() == "Microsoft.DBforPostgreSQL"
            && call.resource_type() == "flexibleServers"
            && call.method() == "PUT"
    }

    fn modify_request(&self, request: &mut AzureRequest) -> Result<()> {
        // Microsoft Entra authentication needs the tenant, which the request doesn't carry.
        let tenant_id = request.call().tenant_id().to_string();
        request.body_mut()["properties"]["authConfig"] = json!({
            "activeDirectoryAuth": "Enabled",
            "passwordAuth": "Enabled",
            "tenantId": tenant_id,
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_iam_auth() {
        let mut request = AwsRequest::new(
            "rds",
            "CreateDBCluster",
            json!({ "DBClusterIdentifier": "db" }),
        );
        Rds.modify_request(&mut request).unwrap();
        assert_eq!(
            request.param("EnableIAMDatabaseAuthentication"),
            Some("true")
        );
    }

    fn modify(path: &str, tf: &str) -> TfModule {
        let module = TfModule::new(
            "postgres-iam-auth",
            indexmap! { path.to_string() => tf.to_string() },
        );
        Terraform.modify(module.unwrap()).unwrap()
    }

    #[test]
    fn turns_on_entra_auth_in_the_tenant_terraform_applies_in() {
        let server =
            "resource \"azurerm_postgresql_flexible_server\" \"db\" {\n  name = \"db\"\n}\n";
        let changed = modify("db.tf", server).changed_files();
        assert_eq!(
            changed["db.tf"],
            "resource \"azurerm_postgresql_flexible_server\" \"db\" {\n  name = \"db\"\n  authentication {\n    active_directory_auth_enabled = true\n    password_auth_enabled         = true\n    tenant_id                     = data.azurerm_client_config.postgres_iam_auth.tenant_id\n  }\n}\n"
        );
        assert_eq!(
            changed["postgres-iam-auth.tf"],
            "data \"azurerm_client_config\" \"postgres_iam_auth\" {\n}\n"
        );
    }

    // The client config reads the azurerm provider, which an AWS or Google Cloud module lacks.
    #[test]
    fn reads_no_azure_client_config_without_an_azure_server() {
        let cluster = "resource \"aws_rds_cluster\" \"db\" {\n}\n";
        let changed = modify("db.tf", cluster).changed_files();
        assert_eq!(changed.keys().collect::<Vec<_>>(), ["db.tf"]);
    }
}
