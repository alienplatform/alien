//! Definite setup-owned workload role overflow after identity mutation.
//!
//! Missing controller names remain unresolved. No hypothetical physical names
//! or live IAM reads are used to claim complete role headroom.

use crate::{
    error::{ErrorData, Result},
    CheckResult,
};
use alien_core::{InitialSetupAuthority, Platform, ResourceLifecycle, ServiceAccount, Stack};
use alien_error::{Context, IntoAlienError};
use alien_permissions::{
    aws_resource_policies::{
        aws_resource_policy_name, aws_service_account_injected_statements,
        render_aws_resource_policy, AwsResourcePolicyContext,
    },
    generators::{AwsIamPolicy, AwsRuntimePermissionsGenerator},
    BindingTarget, PermissionContext,
};
use indexmap::IndexMap;

/// Check only the inline documents DirectSetup will write to frozen workload roles.
pub(crate) fn check(
    stack: &Stack,
    platform: Platform,
    authority: Option<InitialSetupAuthority>,
    context: &PermissionContext,
) -> Result<CheckResult> {
    let mut result = CheckResult::success().with_check_metadata(
        Some("IAM_WORKLOAD_INLINE_AGGREGATE"),
        "Setup workload inline policies fit the aggregate per-role limit",
    );
    if platform != Platform::Aws || authority != Some(InitialSetupAuthority::DirectSetup) {
        return Ok(result);
    }
    let generator = AwsRuntimePermissionsGenerator::new();
    for (profile_name, profile) in stack.permission_profiles() {
        if profile_name == "management" {
            continue;
        }
        let Some(account_entry) = stack.resources.get(&format!("{profile_name}-sa")) else {
            continue;
        };
        let Some(account) = account_entry.config.downcast_ref::<ServiceAccount>() else {
            continue;
        };
        if account_entry.lifecycle != ResourceLifecycle::Frozen
            || account_entry.enabled_when.is_some()
        {
            continue;
        }
        // None replaces an earlier resolved same-name document too: counting
        // the old document would overstate a definite lower bound.
        let mut documents: IndexMap<String, Option<String>> = IndexMap::new();
        let mut statements = Vec::new();
        let mut incomplete = false;
        for set in &account.stack_permission_sets {
            match generator.generate_policy(set, BindingTarget::Stack, context) {
                Ok(policy) => statements.extend(policy.statement),
                Err(_) => incomplete = true,
            }
        }
        statements.extend(aws_service_account_injected_statements(
            account, stack, context,
        ));
        if !statements.is_empty() {
            let document = serde_json::to_string(&AwsIamPolicy {
                version: "2012-10-17".into(),
                statement: statements,
            })
            .into_alien_error()
            .context(ErrorData::CompileTimeCheckFailed {
                check_name: "IAM workload aggregate".into(),
                message: "Could not serialize the stack policy lower bound".into(),
                resource_id: Some(account.id.clone()),
            })?;
            documents.insert("deployment-permissions".into(), Some(document));
        }
        for (id, entry) in stack.resources() {
            if entry.lifecycle != ResourceLifecycle::Frozen || entry.enabled_when.is_some() {
                continue;
            }
            let resource_type = entry.config.resource_type().to_string();
            // These controllers call the shared workload policy helper. Other
            // controllers have distinct layouts and cannot be inferred here.
            if !matches!(
                resource_type.as_str(),
                "storage"
                    | "queue"
                    | "kv"
                    | "build"
                    | "vault"
                    | "worker"
                    | "artifact-registry"
                    | "service-account"
                    | "ai"
                    | "sandbox"
            ) {
                continue;
            }
            let explicit = profile.0.get(id).map(Vec::as_slice).unwrap_or_default();
            let prefix = format!("{resource_type}/");
            let refs = explicit.iter().chain(
                profile
                    .0
                    .get("*")
                    .into_iter()
                    .flatten()
                    .filter(|reference| reference.id().starts_with(&prefix)),
            );
            let selection = AwsResourcePolicyContext {
                named_by_resource: explicit,
                stack_sets: &account.stack_permission_sets,
                generator: &generator,
                permission_context: context,
            };
            for reference in refs {
                let Some(set) =
                    reference.resolve(|name| alien_permissions::get_permission_set(name).cloned())
                else {
                    incomplete = true;
                    continue;
                };
                let name = aws_resource_policy_name(id, &set.id);
                let document = match selection.is_redundant(reference) {
                    Ok(true) => continue,
                    Ok(false) => render_aws_resource_policy(id, &set, &generator, context)
                        .ok()
                        .map(|policy| policy.document),
                    Err(_) => None,
                };
                incomplete |= document.is_none();
                documents.insert(name, document);
            }
        }
        let size: usize = documents
            .values()
            .flatten()
            .map(|document| {
                document
                    .chars()
                    .filter(|c| !c.is_ascii_whitespace())
                    .count()
            })
            .sum();
        if size > 10_240 {
            result.add_error(format!("IAM role for permission profile '{profile_name}' needs at least {size} inline policy characters across setup resources; the aggregate per-role limit is 10240. Reduce grants or use managed policies. https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_iam-quotas.html"));
        }
        if incomplete || !documents.is_empty() {
            result.add_warning(format!("IAM inline policy coverage for permission profile '{profile_name}' is incomplete: only resolved setup workload documents were counted. Controller names, injected grants, remote bindings and retained policies may consume additional quota; passing does not establish total role headroom."));
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{mutations::ServiceAccountMutation, StackMutation};
    use alien_core::{
        DeploymentConfig, PermissionProfile, PermissionSetReference, StackState, Storage,
    };

    fn check(
        stack: &Stack,
        platform: Platform,
        authority: Option<InitialSetupAuthority>,
    ) -> Result<CheckResult> {
        super::check(stack, platform, authority, &PermissionContext::new())
    }

    fn grant(id: &str, count: usize, dynamic: bool) -> PermissionSetReference {
        let resources: Vec<_> = (0..count)
            .map(|index| {
                if dynamic {
                    format!("arn:aws:s3:::${{resourceName}}/objects/object-{index:04}")
                } else {
                    format!("arn:aws:s3:::q5-policy-control/objects/object-{index:04}")
                }
            })
            .collect();
        PermissionSetReference::from_inline(
            serde_json::from_value(serde_json::json!({
                "id": id, "description": "Read synthetic objects",
                "platforms": {"aws": [{"grant": {"actions": ["s3:GetObject"]},
                "binding": {"resource": {"resources": resources}}}]}
            }))
            .unwrap(),
        )
    }

    async fn prepared(count: usize, second: Vec<PermissionSetReference>) -> Stack {
        let mut stack = Stack::new("q5-aggregate".into())
            .add(
                Storage::new("objects-a".into()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Storage::new("objects-b".into()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let mut profile = PermissionProfile::new();
        profile
            .0
            .insert("objects-a".into(), vec![grant("read", count, false)]);
        profile.0.insert("objects-b".into(), second);
        stack.permissions.profiles.insert("reader".into(), profile);
        let config: DeploymentConfig = DeploymentConfig::builder()
            .stack_settings(alien_core::StackSettings::default())
            .environment_variables(alien_core::EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .allow_frozen_changes(false)
            .external_bindings(alien_core::ExternalBindings::default())
            .build();
        ServiceAccountMutation
            .mutate(stack, &StackState::new(Platform::Aws), &config)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn injected_lambda_grants_can_overflow_an_otherwise_fitting_role() {
        let set = serde_json::from_value(serde_json::json!({
            "id": "q5-global", "description": "Synthetic size boundary",
            "platforms": {"aws": [{"grant": {"actions": ["s3:GetObject"]},
                "binding": {"stack": {"resources": [format!("arn:aws:s3:::{}", "x".repeat(9950))]}}}]}
        })).unwrap();
        let mut account = ServiceAccount::new("reader-sa".into()).build();
        account.stack_permission_sets.push(set);
        let mut stack = Stack::new("q5-injected".into())
            .add(account.clone(), ResourceLifecycle::Frozen)
            .add(
                alien_core::Worker::new("handler".into())
                    .permissions("reader".into())
                    .code(alien_core::WorkerCode::Image {
                        image: "q5-image:latest".into(),
                    })
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        stack
            .permissions
            .profiles
            .insert("reader".into(), PermissionProfile::new());
        let context = PermissionContext::new().with_managing_account_id("123456789012");
        let base = AwsRuntimePermissionsGenerator::new()
            .generate_policy(
                &account.stack_permission_sets[0],
                BindingTarget::Stack,
                &context,
            )
            .unwrap();
        let base_size = serde_json::to_string(&base).unwrap().len();
        assert!(base_size <= 10240);
        let injected = aws_service_account_injected_statements(&account, &stack, &context);
        assert_eq!(injected.len(), 2);
        assert_eq!(
            injected[1].resource,
            ["arn:aws:ecr:*:123456789012:repository/*"]
        );
        let mut final_policy = base;
        final_policy.statement.extend(injected);
        let size = serde_json::to_string(&final_policy).unwrap().len();
        assert!(size > 10240);
        let result = super::check(
            &stack,
            Platform::Aws,
            Some(InitialSetupAuthority::DirectSetup),
            &context,
        )
        .unwrap();
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains(&size.to_string()));
        assert!(
            check(
                &stack,
                Platform::Aws,
                Some(InitialSetupAuthority::DirectSetup)
            )
            .unwrap()
            .success
        );
        stack.resources.shift_remove("handler");
        assert!(
            super::check(
                &stack,
                Platform::Aws,
                Some(InitialSetupAuthority::DirectSetup),
                &context
            )
            .unwrap()
            .success
        );
        account.stack_permission_sets.clear();
        assert!(aws_service_account_injected_statements(&account, &stack, &context).is_empty());
    }

    #[tokio::test]
    async fn post_mutation_role_aggregate_rejects_two_fitting_resource_batches() {
        let stack = prepared(100, vec![grant("read", 100, false)]).await;
        let result = check(
            &stack,
            Platform::Aws,
            Some(InitialSetupAuthority::DirectSetup),
        )
        .unwrap();
        assert!(!result.success);
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("10818"), "{:?}", result.errors);
        assert!(result.errors[0].contains("reader"));
        assert_eq!(result.warnings.len(), 1);
        let mut one = stack.clone();
        one.resources.shift_remove("objects-b");
        assert!(
            check(
                &one,
                Platform::Aws,
                Some(InitialSetupAuthority::DirectSetup)
            )
            .unwrap()
            .success
        );
        let small = prepared(10, vec![grant("read", 10, false)]).await;
        assert!(
            check(
                &small,
                Platform::Aws,
                Some(InitialSetupAuthority::DirectSetup)
            )
            .unwrap()
            .success
        );
    }

    #[tokio::test]
    async fn final_same_name_document_replaces_large_or_unresolved_predecessors() {
        for second in [
            vec![grant("read", 200, false), grant("read", 10, false)],
            vec![grant("read", 200, false), grant("read", 200, true)],
            vec![grant("read", 200, true), grant("read", 10, false)],
        ] {
            let stack = prepared(100, second).await;
            let result = check(
                &stack,
                Platform::Aws,
                Some(InitialSetupAuthority::DirectSetup),
            )
            .unwrap();
            assert!(result.success, "{:?}", result.errors);
            assert_eq!(result.warnings.len(), 1);
        }
    }

    #[tokio::test]
    async fn imported_authority_and_live_resources_do_not_count_hypothetical_writes() {
        let mut stack = prepared(100, vec![grant("read", 100, false)]).await;
        assert_eq!(check(&stack, Platform::Aws, None).unwrap().errors.len(), 0);
        for platform in [
            Platform::Local,
            Platform::Kubernetes,
            Platform::Azure,
            Platform::Gcp,
        ] {
            let result = check(&stack, platform, Some(InitialSetupAuthority::DirectSetup)).unwrap();
            assert!(result.success);
            assert!(result.warnings.is_empty());
        }
        stack.resources.get_mut("objects-b").unwrap().lifecycle = ResourceLifecycle::Live;
        assert!(
            check(
                &stack,
                Platform::Aws,
                Some(InitialSetupAuthority::DirectSetup)
            )
            .unwrap()
            .success
        );
        stack.resources.get_mut("objects-a").unwrap().enabled_when = Some("unresolved".into());
        assert!(
            check(
                &stack,
                Platform::Aws,
                Some(InitialSetupAuthority::DirectSetup)
            )
            .unwrap()
            .success
        );
    }
    #[tokio::test]
    async fn implicit_wildcards_are_omitted_but_explicit_refs_still_consume_role_quota() {
        let mut stack = prepared(10, vec![]).await;
        let resources: Vec<_> = (0..100)
            .map(|index| format!("arn:aws:s3:::q5-policy-control/objects/object-{index:04}"))
            .collect();
        let set = serde_json::from_value(serde_json::json!({
            "id": "storage/read", "description": "Read synthetic objects",
            "platforms": {"aws": [{"grant": {"actions": ["s3:GetObject"]},
            "binding": {"resource": {"resources": resources}, "stack": {"resources": resources}}}]}
        }))
        .unwrap();
        let reference = PermissionSetReference::from_inline(set);
        for explicit in [false, true] {
            let mut profile = PermissionProfile::new();
            profile.0.insert("*".into(), vec![reference.clone()]);
            if explicit {
                profile
                    .0
                    .insert("objects-a".into(), vec![reference.clone()]);
            }
            stack.permissions.profiles.insert("reader".into(), profile);
            let config = config();
            let target = ServiceAccountMutation
                .mutate(stack.clone(), &StackState::new(Platform::Aws), &config)
                .await
                .unwrap();
            let result = check(
                &target,
                Platform::Aws,
                Some(InitialSetupAuthority::DirectSetup),
            )
            .unwrap();
            assert_eq!(result.success, !explicit, "{:?}", result.errors);
            assert_eq!(result.errors.len(), usize::from(explicit));
            if explicit {
                assert!(result.errors[0].contains("10832"), "{:?}", result.errors);
            }
        }
    }

    #[tokio::test]
    async fn runner_resolves_deployment_values_without_inventing_resource_names() {
        let mut stack = prepared(10, vec![]).await;
        let resources: Vec<_> = (0..100)
            .map(|index| format!(
                "arn:aws:lambda:${{awsRegion}}:${{awsAccountId}}:function:${{stackPrefix}}-object-{index:04}"
            ))
            .collect();
        let set = serde_json::from_value(serde_json::json!({
            "id": "read", "description": "Synthetic deployment-context grants",
            "platforms": {"aws": [{"grant": {"actions": ["lambda:GetFunction"]},
            "binding": {"resource": {"resources": resources}}}]}
        }))
        .unwrap();
        let reference = PermissionSetReference::from_inline(set);
        let profile = stack.permissions.profiles.get_mut("reader").unwrap();
        profile
            .0
            .insert("objects-a".into(), vec![reference.clone()]);
        profile.0.insert("objects-b".into(), vec![reference]);
        // Without deployment values these documents are unresolved and cannot block.
        assert!(
            check(
                &stack,
                Platform::Aws,
                Some(InitialSetupAuthority::DirectSetup)
            )
            .unwrap()
            .success
        );
        let state = StackState::with_resource_prefix(Platform::Aws, "q5-context".into());
        let client = alien_core::ClientConfig::Aws(Box::new(alien_core::AwsClientConfig {
            account_id: "123456789012".into(),
            region: "us-east-1".into(),
            credentials: alien_core::AwsCredentials::Imds { endpoint: None },
            service_overrides: None,
        }));
        let mut registry = crate::PreflightRegistry::new();
        registry.add_mutation(Box::new(ServiceAccountMutation));
        let runner = crate::runner::PreflightRunner::with_registry(registry);
        let error = runner
            .run_deployment_time_preflights(
                stack.clone(),
                &state,
                &config(),
                &client,
                None,
                None,
                Some(InitialSetupAuthority::DirectSetup),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, "VALIDATION_FAILED");
        assert!(!error.retryable);
        let Some(ErrorData::ValidationFailed { results, .. }) = error.error else {
            panic!("structured results expected");
        };
        let aggregate = results
            .iter()
            .find(|result| result.code.as_deref() == Some("IAM_WORKLOAD_INLINE_AGGREGATE"))
            .unwrap();
        let context = PermissionContext::new()
            .with_aws_account_id("123456789012")
            .with_aws_region("us-east-1")
            .with_stack_prefix("q5-context");
        let generator = AwsRuntimePermissionsGenerator::new();
        let profile = stack.permissions.profiles.get("reader").unwrap();
        let set = profile.0["objects-a"][0].resolve(|_| None).unwrap();
        let document = render_aws_resource_policy("objects-a", &set, &generator, &context)
            .unwrap()
            .document;
        assert!(document
            .contains("arn:aws:lambda:us-east-1:123456789012:function:q5-context-object-0000"));
        let size = 2 * document
            .chars()
            .filter(|c| !c.is_ascii_whitespace())
            .count();
        assert!(size > 10_240);
        assert_eq!(aggregate.errors.len(), 1);
        assert!(
            aggregate.errors[0].contains(&size.to_string()),
            "{:?}",
            aggregate.errors
        );
        assert_eq!(aggregate.warnings.len(), 1);
        // Missing physical names still do not become guessed deployment-prefix names.
        let unresolved = prepared(100, vec![grant("read", 200, true)]).await;
        assert!(
            super::check(
                &unresolved,
                Platform::Aws,
                Some(InitialSetupAuthority::DirectSetup),
                &context
            )
            .unwrap()
            .success
        );
        stack.resources.shift_remove("objects-b");
        assert!(
            runner
                .run_deployment_time_preflights(
                    stack,
                    &state,
                    &config(),
                    &client,
                    None,
                    None,
                    Some(InitialSetupAuthority::DirectSetup),
                )
                .await
                .unwrap()
                .1
                .success
        );
    }

    fn config() -> DeploymentConfig {
        DeploymentConfig::builder()
            .stack_settings(alien_core::StackSettings::default())
            .environment_variables(alien_core::EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .allow_frozen_changes(false)
            .external_bindings(alien_core::ExternalBindings::default())
            .build()
    }

    #[tokio::test]
    async fn deployment_runner_reports_aggregate_before_provisioning_with_exact_authority() {
        let stack = prepared(100, vec![grant("read", 100, false)]).await;
        let mut registry = crate::PreflightRegistry::new();
        registry.add_mutation(Box::new(ServiceAccountMutation));
        let runner = crate::runner::PreflightRunner::with_registry(registry);
        let state = StackState::new(Platform::Aws);
        let client = alien_core::ClientConfig::Local {
            state_directory: "/unused".into(),
        };
        for authority in [
            None,
            Some(InitialSetupAuthority::ImportedHandoff),
            Some(InitialSetupAuthority::DirectSetup),
        ] {
            let outcome = runner
                .run_deployment_time_preflights(
                    stack.clone(),
                    &state,
                    &config(),
                    &client,
                    None,
                    None,
                    authority,
                )
                .await;
            if authority == Some(InitialSetupAuthority::DirectSetup) {
                let error = outcome.unwrap_err();
                assert_eq!(error.code, "VALIDATION_FAILED");
                assert!(!error.retryable);
                let Some(ErrorData::ValidationFailed { results, .. }) = error.error else {
                    panic!("structured results expected");
                };
                let aggregate = results
                    .iter()
                    .find(|result| result.code.as_deref() == Some("IAM_WORKLOAD_INLINE_AGGREGATE"))
                    .unwrap();
                assert_eq!(aggregate.errors.len(), 1);
                assert!(aggregate.errors[0].contains("10818"));
            } else {
                assert!(outcome.unwrap().1.success);
            }
        }
    }
}
