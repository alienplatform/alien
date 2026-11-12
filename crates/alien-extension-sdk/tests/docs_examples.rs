//! The docs' tags and budget examples, written as the docs show them, against the SDK's prelude
//! only.

mod tags {
    use alien_extension_sdk::prelude::*;

    pub const TAGS: &[(&str, &str)] = &[("team", "payments"), ("managed-by", "acme")];

    pub struct RdsTags;

    impl AwsInterceptor for RdsTags {
        fn matches(&self, call: &AwsCall) -> bool {
            call.service() == "rds" && call.operation() == "CreateDBCluster"
        }

        fn modify_request(&self, request: &mut AwsRequest) -> Result<()> {
            for (key, value) in TAGS {
                request.push_member("Tags.Tag", [("Key", *key), ("Value", *value)]);
            }
            Ok(())
        }
    }

    pub struct DynamoDbTags;

    impl AwsInterceptor for DynamoDbTags {
        fn matches(&self, call: &AwsCall) -> bool {
            call.service() == "dynamodb" && call.operation() == "CreateTable"
        }

        fn modify_request(&self, request: &mut AwsRequest) -> Result<()> {
            let body = request.body_mut()?;
            let mut tags = body["Tags"].as_array().cloned().unwrap_or_default();
            tags.extend(
                TAGS.iter()
                    .map(|(key, value)| json!({ "Key": key, "Value": value })),
            );
            body["Tags"] = Value::Array(tags);
            Ok(())
        }
    }

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

    pub fn register(ext: &mut Extension) {
        ext.aws_interceptor(RdsTags);
        ext.aws_interceptor(DynamoDbTags);
        ext.gcp_interceptor(CloudSqlLabels);
        ext.azure_interceptor(AzureTags);
    }

    #[test]
    fn adds_tags_after_alien_tags() {
        let mut request = AwsRequest::new(
            "rds",
            "CreateDBCluster",
            json!({ "DBClusterIdentifier": "db", "Tags.Tag.1.Key": "deployment", "Tags.Tag.1.Value": "stack" }),
        );
        RdsTags.modify_request(&mut request).unwrap();
        assert_eq!(request.param("Tags.Tag.1.Key"), Some("deployment"));
        assert_eq!(request.param("Tags.Tag.2.Key"), Some("team"));
        assert_eq!(request.param("Tags.Tag.3.Value"), Some("acme"));
    }

    #[test]
    fn the_tags_extension_registers() {
        let ext = Extension::new("tags", register);
        assert_eq!(ext.manifest().unwrap().hooks.len(), 4);
    }
}

mod budget {
    use alien_core::hooks::{EmitInput, EmitMethod, HookKind, ImportInput};
    use alien_extension_sdk::prelude::*;

    // ---- the docs' budget/types/src/lib.rs, as written ----

    /// An AWS budget that emails when actual monthly cost passes 80% of the limit.
    #[alien_resource(type = "budget/alert", outputs = BudgetAlertOutputs, lifecycles = [Frozen], holds_data = false)]
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema, bon::Builder)]
    #[serde(rename_all = "camelCase")]
    #[builder(start_fn = new)]
    pub struct BudgetAlert {
        #[builder(start_fn)]
        pub id: String,
        /// The monthly limit, in US dollars.
        pub monthly_limit_usd: u32,
        /// Who gets the alert.
        pub email: String,
    }

    /// What Alien records after initial setup.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
    #[serde(rename_all = "camelCase")]
    pub struct BudgetAlertOutputs {
        pub budget_name: String,
    }

    // ---- the docs' budget/hooks/src/lib.rs, as written ----

    pub struct CloudFormation;

    impl CfEmitter for CloudFormation {
        fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
            let alert = resource_config::<BudgetAlert>(ctx, BudgetAlert::RESOURCE_TYPE)?;
            let mut budget = CfResource::new(
                required_logical_id(ctx)?.to_string(),
                "AWS::Budgets::Budget".into(),
            );
            budget.properties.insert(
                "Budget".into(),
                CfExpression::object([
                    ("BudgetName", stack_name(&alert.id)),
                    ("BudgetType", CfExpression::String("COST".into())),
                    ("TimeUnit", CfExpression::String("MONTHLY".into())),
                    (
                        "BudgetLimit",
                        cf_from_json(json!({ "Amount": alert.monthly_limit_usd, "Unit": "USD" }))?,
                    ),
                ]),
            );
            budget.properties.insert(
                "NotificationsWithSubscribers".into(),
                cf_from_json(json!([{
                    "Notification": {
                        "NotificationType": "ACTUAL",
                        "ComparisonOperator": "GREATER_THAN",
                        "Threshold": 80,
                        "ThresholdType": "PERCENTAGE",
                    },
                    "Subscribers": [{ "SubscriptionType": "EMAIL", "Address": alert.email }],
                }]))?,
            );
            Ok(vec![budget])
        }

        /// What the importer reads after initial setup. Ref on a budget is its name.
        fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<CfExpression> {
            Ok(CfExpression::object([(
                "budgetName",
                CfExpression::ref_(required_logical_id(ctx)?),
            )]))
        }
    }

    pub struct Terraform;

    impl TfEmitter for Terraform {
        fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
            let alert = downcast::<BudgetAlert>(ctx, BudgetAlert::RESOURCE_TYPE)?;
            let label = required_label(ctx)?;
            Ok(TfFragment::default().with_resource(resource_block(
                "aws_budgets_budget",
                label,
                [
                    attr("name", resource_prefix_template(&alert.id)),
                    attr("budget_type", Expression::String("COST".into())),
                    attr("time_unit", Expression::String("MONTHLY".into())),
                    attr(
                        "limit_amount",
                        Expression::String(alert.monthly_limit_usd.to_string()),
                    ),
                    attr("limit_unit", Expression::String("USD".into())),
                    nested(block(
                        "notification",
                        [
                            attr("notification_type", Expression::String("ACTUAL".into())),
                            attr(
                                "comparison_operator",
                                Expression::String("GREATER_THAN".into()),
                            ),
                            attr("threshold", Expression::Number(80.into())),
                            attr("threshold_type", Expression::String("PERCENTAGE".into())),
                            attr(
                                "subscriber_email_addresses",
                                Expression::Array(vec![Expression::String(alert.email.clone())]),
                            ),
                        ],
                    )),
                ],
            )))
        }

        fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
            let label = required_label(ctx)?;
            Ok(expr::object([(
                "budgetName",
                expr::traversal(["aws_budgets_budget", label, "name"]),
            )]))
        }
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct BudgetImport {
        pub budget_name: String,
    }

    pub struct Importer;

    impl ResourceImporter for Importer {
        type ImportData = BudgetImport;

        fn import(
            &self,
            data: BudgetImport,
            ctx: &ImportContext<'_>,
        ) -> Result<StackResourceState> {
            // No controller, so there are no fields to save, only the outputs.
            ctx.ready(
                (),
                BudgetAlertOutputs {
                    budget_name: data.budget_name,
                },
            )
        }
    }

    pub fn register(ext: &mut Extension) {
        ext.resource::<BudgetAlert>()
            .cloudformation_emitter(Platform::Aws, CloudFormation)
            .terraform_emitter(Platform::Aws, Terraform)
            .importer(Platform::Aws, Importer);
    }

    fn alert_entry() -> Value {
        json!({
            "config": { "type": "budget/alert", "id": "costs", "monthlyLimitUsd": 500, "email": "a@b.c" },
            "lifecycle": "frozen",
            "dependencies": []
        })
    }

    /// Runs the extension's hook of `kind` natively, as the module does.
    fn call(kind: HookKind, input: impl Serialize) -> Value {
        let ext = Extension::new("budget", register);
        let hook = ext
            .manifest()
            .unwrap()
            .hooks
            .into_iter()
            .find(|entry| entry.kind == kind)
            .unwrap()
            .name;
        let output = ext.call(&hook, &serde_json::to_vec(&input).unwrap());
        serde_json::from_slice(&output.unwrap_or_else(|error| panic!("{error}"))).unwrap()
    }

    fn emit(method: EmitMethod) -> EmitInput {
        EmitInput {
            method,
            resource_id: "costs".into(),
            platform: Platform::Aws,
            targets_kubernetes: false,
            stack_settings: Default::default(),
            names: [("costs".to_string(), "Costs".to_string())]
                .into_iter()
                .collect(),
            resource: alert_entry(),
        }
    }

    #[test]
    fn the_budget_emitters_run_as_the_docs_write_them() {
        let resources = call(HookKind::CloudformationEmitter, emit(EmitMethod::Emit));
        let budget = &resources[0];
        assert_eq!(budget[0], "Costs");
        assert_eq!(budget[1]["Type"], "AWS::Budgets::Budget");
        assert_eq!(
            budget[1]["Properties"]["Budget"]["BudgetName"],
            json!({ "Fn::Sub": "${AWS::StackName}-costs" })
        );
        assert_eq!(
            budget[1]["Properties"]["Budget"]["BudgetLimit"],
            json!({ "Amount": 500, "Unit": "USD" })
        );
        assert_eq!(
            budget[1]["Properties"]["NotificationsWithSubscribers"][0]["Subscribers"][0]["Address"],
            "a@b.c"
        );
        assert_eq!(
            call(
                HookKind::CloudformationEmitter,
                emit(EmitMethod::EmitImportRef)
            ),
            json!({ "budgetName": { "Ref": "Costs" } })
        );

        let fragment = call(HookKind::TerraformEmitter, emit(EmitMethod::Emit));
        let blocks = fragment["resourceBlocks"].as_str().unwrap();
        assert!(
            blocks.contains("resource \"aws_budgets_budget\" \"Costs\""),
            "{blocks}"
        );
        assert!(blocks.contains("limit_amount = \"500\""), "{blocks}");
        assert!(blocks.contains("notification {"), "{blocks}");
        let import_ref = call(HookKind::TerraformEmitter, emit(EmitMethod::EmitImportRef));
        assert!(
            import_ref
                .as_str()
                .unwrap()
                .contains("aws_budgets_budget.Costs.name"),
            "{import_ref}"
        );
    }

    #[test]
    fn the_budget_importer_runs_as_the_docs_write_it() {
        let output = call(
            HookKind::Importer,
            ImportInput {
                resource_id: "costs".into(),
                platform: Platform::Aws,
                region: "us-east-1".into(),
                stack_settings: Default::default(),
                management_config: None,
                resource: alert_entry(),
                import_data: json!({ "budgetName": "stack-costs" }),
            },
        );
        // No controller, so the state saves no fields.
        assert_eq!(output["internalState"], json!({ "state": "ready" }));
        assert_eq!(output["status"], "running");
        assert_eq!(output["outputs"], json!({ "budgetName": "stack-costs" }));
    }
}
