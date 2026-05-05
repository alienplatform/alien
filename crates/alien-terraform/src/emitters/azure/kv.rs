//! Azure KV — Cosmos DB account in NoSQL (Core/SQL) mode.
//!
//! The runtime controller (`AzureKvController`) currently uses Azure Table
//! Storage backed by the shared storage account. The Distribution rebuild
//! homogenises the import surface around Cosmos DB instead — the
//! `AzureKvImportData` shape (`accountName` + `databaseName` +
//! `containerName` + `endpoint`) is purpose-built for that. So the TF
//! emitter materialises a Cosmos DB account plus a database plus a
//! container per `Kv` resource, and the importer + runtime will converge
//! on Cosmos DB once the controller is reworked (out of scope here).
//!
//! Defaults closed by design:
//!
//! * Single-region (`var.azure_location`), session consistency.
//! * Disable local auth — RBAC is the only authentication path.
//! * Backup retained 8 hours.
//!
//! The container uses `/pk` as the partition-key path so the controller's
//! single-collection schema continues to work.

use crate::{
    block::{attr, block, nested, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::azure::helpers::{downcast, required_label, stack_name_template, tags},
    expr,
};
use alien_core::{import::EmitContext, Kv, Result};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AzureKvEmitter;

impl TfEmitter for AzureKvEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let kv = downcast::<Kv>(ctx, Kv::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let database_label = format!("{label}_db");
        let container_label = format!("{label}_container");

        let mut fragment = TfFragment::default();

        fragment.resource_blocks.push(resource_block(
            "azurerm_cosmosdb_account",
            label,
            [
                attr("name", account_name_expr(kv.id())),
                attr(
                    "resource_group_name",
                    expr::raw("var.azure_resource_group_name"),
                ),
                attr("location", expr::raw("var.azure_location")),
                attr("offer_type", Expression::String("Standard".to_string())),
                attr("kind", Expression::String("GlobalDocumentDB".to_string())),
                attr("local_authentication_disabled", Expression::Bool(true)),
                nested(block(
                    "consistency_policy",
                    [attr(
                        "consistency_level",
                        Expression::String("Session".to_string()),
                    )],
                )),
                nested(block(
                    "geo_location",
                    [
                        attr("location", expr::raw("var.azure_location")),
                        attr(
                            "failover_priority",
                            Expression::Number(hcl::Number::from(0i64)),
                        ),
                    ],
                )),
                nested(block(
                    "backup",
                    [
                        attr("type", Expression::String("Periodic".to_string())),
                        attr(
                            "interval_in_minutes",
                            Expression::Number(hcl::Number::from(240i64)),
                        ),
                        attr(
                            "retention_in_hours",
                            Expression::Number(hcl::Number::from(8i64)),
                        ),
                        attr(
                            "storage_redundancy",
                            Expression::String("Local".to_string()),
                        ),
                    ],
                )),
                attr("tags", tags(ctx, "kv")),
            ],
        ));

        fragment.resource_blocks.push(resource_block(
            "azurerm_cosmosdb_sql_database",
            &database_label,
            [
                attr("name", stack_name_template(&format!("{}-db", kv.id()))),
                attr(
                    "resource_group_name",
                    expr::traversal(["azurerm_cosmosdb_account", label, "resource_group_name"]),
                ),
                attr(
                    "account_name",
                    expr::traversal(["azurerm_cosmosdb_account", label, "name"]),
                ),
            ],
        ));

        fragment.resource_blocks.push(resource_block(
            "azurerm_cosmosdb_sql_container",
            &container_label,
            [
                attr("name", stack_name_template(kv.id())),
                attr(
                    "resource_group_name",
                    expr::traversal(["azurerm_cosmosdb_account", label, "resource_group_name"]),
                ),
                attr(
                    "account_name",
                    expr::traversal(["azurerm_cosmosdb_account", label, "name"]),
                ),
                attr(
                    "database_name",
                    expr::traversal(["azurerm_cosmosdb_sql_database", &database_label, "name"]),
                ),
                attr(
                    "partition_key_paths",
                    Expression::Array(vec![Expression::String("/pk".to_string())]),
                ),
                attr(
                    "partition_key_version",
                    Expression::Number(hcl::Number::from(2i64)),
                ),
            ],
        ));

        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        let database_label = format!("{label}_db");
        let container_label = format!("{label}_container");
        Ok(expr::object([
            ("subscriptionId", expr::raw("var.azure_subscription_id")),
            ("resourceGroup", expr::raw("var.azure_resource_group_name")),
            (
                "accountName",
                expr::traversal(["azurerm_cosmosdb_account", label, "name"]),
            ),
            (
                "databaseName",
                expr::traversal(["azurerm_cosmosdb_sql_database", &database_label, "name"]),
            ),
            (
                "containerName",
                expr::traversal(["azurerm_cosmosdb_sql_container", &container_label, "name"]),
            ),
            (
                "endpoint",
                expr::traversal(["azurerm_cosmosdb_account", label, "endpoint"]),
            ),
        ]))
    }
}

/// Cosmos DB account names are 3-50 chars, lowercase alphanumeric, and
/// hyphens are allowed but the prefix must avoid leading/trailing dashes.
/// `${var.stack_name}-{id}` lower-cased is good enough for our naming
/// scheme — runtime importers will discover the same name on the other
/// end.
fn account_name_expr(kv_id: &str) -> Expression {
    expr::raw(format!("lower(\"${{var.stack_name}}-{}\")", kv_id))
}
