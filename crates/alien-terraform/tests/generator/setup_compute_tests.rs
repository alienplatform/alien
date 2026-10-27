//! Compare installer choices with real prepared packages, without cloud resources.
use alien_cloudformation::{
    generate_cloudformation_template, CfEmitter, CfExpression, CfRegistry, CfResource,
    CloudFormationOptions, CloudFormationTarget, RegistrationMode,
};
use alien_core::{
    compute_planner::{plan_compute, plan_setup_compute},
    import::EmitContext,
    CapacityGroup, CapacityGroupScalePolicy, ComputeChoiceRange, ComputeCluster,
    ComputePoolSelection, ComputeSettings, Container, ContainerAutoscaling, ContainerCode,
    DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings, Platform, ResourceLifecycle,
    ResourceSpec, Result, Stack, StackSettings, StackState,
};
use alien_preflights::{mutations::ComputeClusterMutation, StackMutation};
use alien_terraform::{
    expr, generate_terraform_module, TerraformOptions, TerraformTarget, TfEmitter, TfFragment,
    TfRegistry,
};
use hcl::Expression;
use serde_json::json;

use super::helpers::{linter_files, test_utils};

// The generators own compute parameters and registration settings. These
// extension emitters supply only the resource/import reference they require.
struct ComputeEmitter;
impl CfEmitter for ComputeEmitter {
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        Ok(vec![CfResource::new(
            ctx.name_for(ctx.resource_id).unwrap().to_string(),
            "AWS::CloudFormation::WaitConditionHandle".into(),
        )])
    }
    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<CfExpression> {
        Ok(CfExpression::object([(
            "clusterId",
            CfExpression::ref_(ctx.name_for(ctx.resource_id).unwrap()),
        )]))
    }
}
impl TfEmitter for ComputeEmitter {
    fn emit(&self, _ctx: &EmitContext<'_>) -> Result<TfFragment> {
        Ok(TfFragment::empty())
    }
    fn emit_import_ref(&self, _ctx: &EmitContext<'_>) -> Result<Expression> {
        Ok(expr::object([(
            "clusterId",
            Expression::String("acme".into()),
        )]))
    }
}

fn source(containers: u32, explicit: bool) -> Stack {
    let mut stack = Stack::new("acme".into());
    for i in 0..containers {
        let mut container = Container::new(format!("api-{i}"))
            .code(ContainerCode::Image {
                image: "example.com/acme:1".into(),
            })
            .cpu(ResourceSpec {
                min: "1".into(),
                desired: "1".into(),
            })
            .memory(ResourceSpec {
                min: "1Gi".into(),
                desired: "1Gi".into(),
            })
            .permissions("app".into())
            .build();
        container.autoscaling = Some(ContainerAutoscaling {
            min: 1,
            desired: 1,
            max: 3,
            target_cpu_percent: None,
            target_memory_percent: None,
            target_http_in_flight_per_replica: None,
            max_http_p95_latency_ms: None,
        });
        stack = stack.add(container, ResourceLifecycle::Live);
    }
    if explicit {
        stack = stack.add(
            ComputeCluster::new("compute".into())
                .capacity_group(CapacityGroup {
                    group_id: "general".into(),
                    instance_type: None,
                    profile: None,
                    min_size: 1,
                    max_size: 3,
                    scale_policy: Some(CapacityGroupScalePolicy::Autoscale {
                        min: ComputeChoiceRange {
                            min: 1,
                            max: 2,
                            default: 1,
                        },
                        max: ComputeChoiceRange {
                            min: 3,
                            max: 5,
                            default: 3,
                        },
                    }),
                    nested_virtualization: None,
                })
                .build(),
            ResourceLifecycle::Frozen,
        );
    }
    stack.build()
}

#[tokio::test]
async fn single_implicit_pool_matches_setup_artifacts() {
    compare(1, false).await;
}
#[tokio::test]
async fn multiple_implicit_workloads_match_setup_artifacts() {
    compare(2, false).await;
}
#[tokio::test]
async fn explicit_pool_setup_bounds_stay_declared() {
    compare(1, true).await;
}
#[tokio::test]
async fn large_implicit_pool_matches_package_recommendation() {
    compare(50, false).await;
}

async fn compare(containers: u32, explicit: bool) {
    let source = source(containers, explicit);
    let baseline = plan_compute(&source, Platform::Aws, None).unwrap();
    let settings = StackSettings {
        compute: Some(ComputeSettings {
            pools: baseline
                .pools
                .iter()
                .map(|p| (p.pool_id.clone(), p.recommended.clone()))
                .collect(),
            ..Default::default()
        }),
        ..Default::default()
    };
    let config = DeploymentConfig::builder()
        .stack_settings(settings.clone())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: String::new(),
            created_at: "2026-01-01T00:00:00Z".into(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build();
    let prepared = ComputeClusterMutation
        .mutate(source.clone(), &StackState::new(Platform::Aws), &config)
        .await
        .unwrap();
    let mut cf_registry = CfRegistry::built_in();
    cf_registry.register(ComputeCluster::RESOURCE_TYPE, Platform::Aws, ComputeEmitter);
    let template = generate_cloudformation_template(
        &prepared,
        CloudFormationOptions {
            registry: &cf_registry,
            target: CloudFormationTarget::Aws,
            stack_settings: settings.clone(),
            setup_target: "aws".into(),
            setup_fingerprint: "fixture".into(),
            setup_fingerprint_version: 1,
            registration: RegistrationMode::OutputsFallback,
            description: None,
            management_trust: Default::default(),
        },
    )
    .unwrap();
    let rendered = serde_json::to_value(template).unwrap();
    let parameters = &rendered["Parameters"];
    let expected_max = if explicit {
        3
    } else {
        (containers * 3).min(100)
    };
    assert_eq!(parameters["ComputeGeneralMin"]["MinValue"], 1);
    assert_eq!(
        parameters["ComputeGeneralMin"]["MaxValue"],
        if explicit { 2 } else { 1 }
    );
    assert_eq!(parameters["ComputeGeneralMax"]["MinValue"], expected_max);
    assert_eq!(
        parameters["ComputeGeneralMax"]["MaxValue"],
        if explicit { 5 } else { expected_max }
    );

    let plan = plan_setup_compute(&source, Platform::Aws, None).unwrap();
    assert_eq!(plan.pools.len(), 1);
    let CapacityGroupScalePolicy::Autoscale { min, max } = &plan.pools[0].scale else {
        panic!("expected autoscaling setup")
    };
    assert_eq!(
        json!([min.min, min.max, max.min, max.max]),
        json!([
            parameters["ComputeGeneralMin"]["MinValue"],
            parameters["ComputeGeneralMin"]["MaxValue"],
            parameters["ComputeGeneralMax"]["MinValue"],
            parameters["ComputeGeneralMax"]["MaxValue"],
        ])
    );
    let machine = baseline.pools[0].recommended.machine().unwrap();
    assert!(parameters["ComputeGeneralMachine"]["AllowedValues"]
        .as_array()
        .unwrap()
        .contains(&json!(machine)));
    let accepts = |name: &str, value: u32| {
        let p = &parameters[name];
        p["MinValue"].as_u64().unwrap() <= u64::from(value)
            && u64::from(value) <= p["MaxValue"].as_u64().unwrap()
    };
    let mut valid = Vec::new();
    let rejected_fixed = if containers > 2 { expected_max - 1 } else { 1 };
    for (min, max, fixed) in [
        (rejected_fixed, rejected_fixed, true),
        (1, expected_max, false),
        (2, expected_max, false),
        (1, expected_max + 1, false),
    ] {
        let selection = if fixed {
            ComputePoolSelection::Fixed {
                machines: min,
                machine: Some(machine.into()),
                failure_domains: None,
            }
        } else {
            ComputePoolSelection::Autoscale {
                min,
                max,
                machine: Some(machine.into()),
                failure_domains: None,
            }
        };
        let compute = ComputeSettings {
            pools: [("general".into(), selection)].into(),
            ..Default::default()
        };
        let planned = plan_setup_compute(&source, Platform::Aws, Some(&compute)).unwrap();
        let accepted = accepts("ComputeGeneralMin", min) && accepts("ComputeGeneralMax", max);
        assert_eq!(
            planned.pools[0].errors.is_empty(),
            accepted,
            "{min}/{max}: {:?}",
            planned.pools[0].errors
        );
        if accepted {
            valid.push(serde_json::to_value(compute).unwrap());
        }
    }
    assert!(!valid.is_empty());
    // Terraform carries compute through the JSON settings interface, without
    // numeric variables. Execute the generated module and check registration.
    let mut tf_registry = TfRegistry::built_in();
    tf_registry.register(ComputeCluster::RESOURCE_TYPE, Platform::Aws, ComputeEmitter);
    let module = generate_terraform_module(
        &prepared,
        TerraformTarget::Aws,
        TerraformOptions {
            display_name: None,
            registry: &tf_registry,
            stack_settings: settings,
            registration: None,
            helm_install: None,
            supported_aws_regions: Vec::new(),
        },
    )
    .unwrap();
    let mut files = linter_files(&module);
    let mut tests = String::from("mock_provider \"aws\" {}\nmock_provider \"random\" {}\nvariables {\nname = \"acme\"\ntoken = \"test\"\nresource_prefix = \"acme\"\n}\n");
    for (i, compute) in valid.iter().enumerate() {
        let overlay = json!({"compute": compute}).to_string();
        tests.push_str(&format!(r#"
run "accepted_{i}" {{
  command = plan
  variables {{ advanced_settings_overlay_json = {overlay} }}
  assert {{
    condition = local.deployment_settings.compute.pools.general.min == {min} && local.deployment_settings.compute.pools.general.max == {max}
    error_message = "The selected compute must reach registration unchanged."
  }}
}}
"#, overlay = serde_json::to_string(&overlay).unwrap(), min=compute["pools"]["general"]["min"], max=compute["pools"]["general"]["max"]));
    }
    files.insert("compute.tftest.hcl".into(), tests);
    test_utils::terraform_test(&files).assert_ok("setup compute registration plans");
}

#[test]
fn resource_choices_do_not_move_package_bounds() {
    let mut stack = source(2, false);
    let container = stack
        .resources
        .get_mut("api-1")
        .unwrap()
        .config
        .downcast_mut::<Container>()
        .unwrap();
    container.resource_choices = Some(alien_core::ContainerResourceChoices {
        cpu: Some(alien_core::ResourceChoiceRange {
            min: "1".into(),
            max: "4".into(),
            default: "1".into(),
        }),
        memory: Some(alien_core::ResourceChoiceRange {
            min: "1Gi".into(),
            max: "4Gi".into(),
            default: "1Gi".into(),
        }),
    });
    let settings: ComputeSettings = serde_json::from_value(json!({
        "containers": { "api-1": {"cpu": 4, "memory": "4Gi"} },
        "pools": { "general": { "mode": "autoscale", "min": 1, "max": 6 } }
    }))
    .unwrap();
    let plan = plan_setup_compute(&stack, Platform::Aws, Some(&settings)).unwrap();
    assert_eq!(
        plan.pools[0].scale,
        CapacityGroupScalePolicy::from_selected_bounds(1, 6)
    );
    assert!(
        plan.pools[0].errors.is_empty(),
        "{:?}",
        plan.pools[0].errors
    );
    // The existing runtime contract remains broader, including saved counts
    // above the offered range and counts chosen before replica changes.
    let saved: ComputeSettings = serde_json::from_value(json!({
        "pools": { "general": { "mode": "autoscale", "min": 1, "max": 150 } }
    }))
    .unwrap();
    let runtime = plan_compute(&stack, Platform::Aws, Some(&saved)).unwrap();
    assert!(runtime.pools[0].errors.is_empty());
    assert_eq!(runtime.pools[0].selected.max_size(), 150);
}
