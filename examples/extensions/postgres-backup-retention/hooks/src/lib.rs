use alien_extension_sdk::prelude::*;

/// Keeps every RDS cluster's backups for 30 days.
struct CloudFormation;

impl CloudFormationModifier for CloudFormation {
    fn modify(&self, mut template: CfTemplate) -> Result<CfTemplate> {
        for cluster in template.resources_of_type_mut("AWS::RDS::DBCluster") {
            cluster.set_property("BackupRetentionPeriod", 30);
        }
        Ok(template)
    }
}

/// The same retention in the Terraform module, on each cloud.
struct Terraform;

impl TerraformModifier for Terraform {
    fn modify(&self, mut module: TfModule) -> Result<TfModule> {
        for cluster in module.resources_of_type_mut("aws_rds_cluster") {
            cluster.set_attribute("backup_retention_period", 30);
        }
        for instance in module.resources_of_type_mut("google_sql_database_instance") {
            instance
                .block_mut("settings")?
                .block_mut("backup_configuration")?
                .add_block("backup_retention_settings")
                .set_attribute("retained_backups", 30);
        }
        for server in module.resources_of_type_mut("azurerm_postgresql_flexible_server") {
            server.set_attribute("backup_retention_days", 30);
        }
        Ok(module)
    }
}

fn register(ext: &mut Extension) {
    ext.cloudformation_modifier(CloudFormation);
    ext.terraform_modifier(Terraform);
}

alien_extension_sdk::export!("backups", register);
