use super::*;

pub(super) fn add_custom_resource(
    template: &mut CfTemplate,
    service_token: CfExpression,
    management_config: CfExpression,
    stack_settings: CfExpression,
    options: &CloudFormationOptions<'_>,
    resources: CfExpression,
    input_values: CfExpression,
    callback_url: Option<&str>,
) {
    let depends_on = template
        .resources
        .iter()
        .filter_map(|(logical_id, resource)| {
            resource.condition.is_none().then_some(logical_id.clone())
        })
        .collect();
    let mut resource = CfResource::new(
        "DeploymentRegistration".to_string(),
        "AWS::CloudFormation::CustomResource".to_string(),
    );
    resource.depends_on = depends_on;
    // Emit an explicit name so registered deployments mirror the CFN stack list
    // verbatim; callers who want a different identity can rewire the property
    // post-rendering.
    resource.properties = indexmap! {
        "ServiceToken".to_string() => service_token,
        "Token".to_string() => CfExpression::ref_(PARAM_TOKEN),
        "DeploymentName".to_string() => CfExpression::ref_("AWS::StackName"),
        "ResourcePrefix".to_string() => CfExpression::ref_("AWS::StackName"),
        "SourceKind".to_string() => CfExpression::from("cloudformation"),
        "Platform".to_string() => CfExpression::from(options.target.deployment_platform().as_str()),
        "Region".to_string() => CfExpression::ref_("AWS::Region"),
        "SetupTarget".to_string() => CfExpression::from(options.setup_target.clone()),
        "SetupImportFormatVersion".to_string() => CfExpression::from(CURRENT_SETUP_IMPORT_FORMAT_VERSION),
        "SetupFingerprint".to_string() => CfExpression::from(options.setup_fingerprint.clone()),
        "SetupFingerprintVersion".to_string() => CfExpression::from(options.setup_fingerprint_version),
        "ManagementConfig".to_string() => management_config,
        "StackSettings".to_string() => stack_settings,
        "Resources".to_string() => resources,
    };
    if !matches!(&input_values, CfExpression::Object(values) if values.is_empty()) {
        resource
            .properties
            .insert("InputValues".to_string(), input_values);
    }
    if let Some(base_platform) = options.target.base_platform() {
        resource.properties.insert(
            "BasePlatform".to_string(),
            CfExpression::from(base_platform.as_str()),
        );
    }
    if let Some(callback_url) = callback_url.filter(|value| !value.is_empty()) {
        resource.properties.insert(
            "CallbackUrl".to_string(),
            CfExpression::from(callback_url.to_string()),
        );
    }
    template
        .resources
        .insert(resource.logical_id.clone(), resource);
}

pub(super) fn add_outputs(
    template: &mut CfTemplate,
    management_config: CfExpression,
    stack_settings: CfExpression,
    options: &CloudFormationOptions<'_>,
    resources: &[RegistrationEntry],
) -> Result<()> {
    template.outputs.insert(
        OUTPUT_SOURCE_KIND.to_string(),
        output("Setup source kind.", CfExpression::from("cloudformation")),
    );
    if !matches!(options.registration, RegistrationMode::OutputsFallback) {
        template.outputs.insert(
            OUTPUT_DEPLOYMENT_ID.to_string(),
            output(
                "Registered deployment ID.",
                CfExpression::get_att("DeploymentRegistration", "DeploymentId"),
            ),
        );
    }
    template.outputs.insert(
        OUTPUT_RESOURCE_PREFIX.to_string(),
        output(
            "Stable physical resource prefix.",
            CfExpression::ref_("AWS::StackName"),
        ),
    );
    template.outputs.insert(
        OUTPUT_PLATFORM.to_string(),
        output(
            "Target platform.",
            CfExpression::from(options.target.deployment_platform().as_str()),
        ),
    );
    if let Some(base_platform) = options.target.base_platform() {
        template.outputs.insert(
            OUTPUT_BASE_PLATFORM.to_string(),
            output(
                "Base cloud platform for a Kubernetes deployment.",
                CfExpression::from(base_platform.as_str()),
            ),
        );
    }
    template.outputs.insert(
        OUTPUT_REGION.to_string(),
        output("AWS region.", CfExpression::ref_("AWS::Region")),
    );
    template.outputs.insert(
        OUTPUT_SETUP_TARGET.to_string(),
        output(
            "Setup target.",
            CfExpression::from(options.setup_target.clone()),
        ),
    );
    template.outputs.insert(
        OUTPUT_SETUP_FINGERPRINT.to_string(),
        output(
            "Setup compatibility fingerprint.",
            CfExpression::from(options.setup_fingerprint.clone()),
        ),
    );
    template.outputs.insert(
        OUTPUT_SETUP_IMPORT_FORMAT_VERSION.to_string(),
        output(
            "Setup registration payload format version.",
            CfExpression::from(CURRENT_SETUP_IMPORT_FORMAT_VERSION),
        ),
    );
    template.outputs.insert(
        OUTPUT_SETUP_FINGERPRINT_VERSION.to_string(),
        output(
            "Setup fingerprint algorithm version.",
            CfExpression::from(options.setup_fingerprint_version),
        ),
    );
    template.outputs.insert(
        OUTPUT_MANAGEMENT_CONFIG.to_string(),
        output(
            "Deployment registration management configuration JSON.",
            json_output_value(management_config),
        ),
    );
    template.outputs.insert(
        OUTPUT_STACK_SETTINGS.to_string(),
        output(
            "Deployment registration settings JSON.",
            CfExpression::to_json_string(stack_settings),
        ),
    );
    add_resource_outputs(template, resources)?;
    Ok(())
}

fn json_output_value(value: CfExpression) -> CfExpression {
    match value {
        CfExpression::Null => CfExpression::from("null"),
        value => CfExpression::to_json_string(value),
    }
}

/// One resource's registration entry, kept beside the gate that decides whether
/// the deployer wanted it.
///
/// The two registration paths need different forms of the same two pieces, so
/// the gate stays separate until each one renders it: the custom resource gates
/// the entry object, the Outputs fallback gates the entry's JSON text.
pub(super) struct RegistrationEntry {
    /// Stack input id the deployer answers, or `None` when the resource is
    /// created unconditionally.
    pub(super) enabled_when: Option<String>,
    pub(super) entry: CfExpression,
}

impl RegistrationEntry {
    /// Gates a rendered value on this entry's input: a declined entry
    /// resolves to `AWS::NoValue`, which removes the element from its list.
    fn gated(&self, value: CfExpression) -> CfExpression {
        enabled::when_enabled(
            self.enabled_when.as_deref(),
            value,
            CfExpression::no_value(),
        )
    }

    /// Element for the custom resource's `Resources` property.
    pub(super) fn custom_resource_element(&self) -> CfExpression {
        self.gated(self.entry.clone())
    }

    /// The entry's JSON text for the Outputs fallback, gated so a declined entry
    /// resolves to `AWS::NoValue`.
    ///
    /// `Fn::ToJsonString` sits *inside* the gate on purpose. Wrapping the gate
    /// instead — `ToJsonString(Fn::If(..))` — serializes a declined entry as the
    /// literal `null`, which is the bug this construction exists to avoid.
    ///
    /// The `Fn::Sub` around it only launders the type. CloudFormation resolves a
    /// bare `Fn::ToJsonString` inside `Fn::Join` correctly, but cfn-lint does not
    /// infer that it returns a string and fails the template with E6101, and the
    /// only way to silence that is a template-wide suppression that would also
    /// mask real E6101s. Substitution is a single pass, so JSON containing a
    /// literal `${...}` passes through untouched.
    fn outputs_element(&self) -> CfExpression {
        let json_text = CfExpression::sub_with(
            format!("${{{ENTRY_JSON_SUB_VARIABLE}}}"),
            [(
                ENTRY_JSON_SUB_VARIABLE,
                CfExpression::to_json_string(self.entry.clone()),
            )],
        );
        self.gated(json_text)
    }
}

/// Renders a chunk of entries as the JSON array text a stack output carries.
///
/// With no gated entry this is `Fn::ToJsonString` over the whole list, which
/// keeps an ungated stack's output unchanged on re-apply.
///
/// Once any entry is gated that no longer works. `Fn::ToJsonString` does not
/// honour `AWS::NoValue`: a declined entry survives as a literal `null`, and
/// registration runs the typed importer over every element it receives, so the
/// null fails deserialization rather than being skipped. `Fn::Join` does drop
/// `AWS::NoValue` elements — without leaving a stray delimiter, and collapsing
/// to `[]` when every entry is declined — so gated chunks convert each entry to
/// JSON text on its own and join the array together.
fn resources_json(entries: &[RegistrationEntry]) -> CfExpression {
    if entries.iter().all(|entry| entry.enabled_when.is_none()) {
        return CfExpression::to_json_string(CfExpression::list(
            entries.iter().map(|entry| entry.entry.clone()),
        ));
    }

    CfExpression::join(
        "",
        CfExpression::list([
            CfExpression::from("["),
            CfExpression::join(
                ",",
                CfExpression::list(entries.iter().map(RegistrationEntry::outputs_element)),
            ),
            CfExpression::from("]"),
        ]),
    )
}

fn add_resource_outputs(template: &mut CfTemplate, entries: &[RegistrationEntry]) -> Result<()> {
    let chunks = chunk_registration_entries(entries)?;
    if STANDARD_OUTPUT_COUNT + chunks.len() > CLOUDFORMATION_MAX_OUTPUTS {
        return Err(AlienError::new(ErrorData::OperationNotSupported {
            operation: "generate_cloudformation_template".to_string(),
            reason: format!(
                "CloudFormation Outputs fallback needs {} resource chunks, exceeding the {} output limit",
                chunks.len(),
                CLOUDFORMATION_MAX_OUTPUTS
            ),
        }));
    }

    if chunks.len() == 1 {
        let chunk = chunks.into_iter().next().expect("one chunk");
        let value = if chunk.is_empty() {
            CfExpression::from("[]")
        } else {
            resources_json(chunk)
        };
        template.outputs.insert(
            OUTPUT_RESOURCES.to_string(),
            output("Deployment registration resources JSON.", value),
        );
        return Ok(());
    }

    for (index, chunk) in chunks.into_iter().enumerate() {
        template.outputs.insert(
            format!("{OUTPUT_RESOURCES}{index}"),
            output(
                "Deployment registration resources JSON chunk. Reassemble chunks in numeric suffix order.",
                resources_json(chunk),
            ),
        );
    }

    Ok(())
}

pub(super) fn empty_object() -> CfExpression {
    CfExpression::Object(IndexMap::new())
}

pub(super) fn kubernetes_cluster_namespace(stack: &Stack) -> Option<String> {
    stack.resources().find_map(|(_resource_id, entry)| {
        entry
            .config
            .downcast_ref::<KubernetesCluster>()
            .map(|cluster| cluster.namespace.clone())
    })
}

/// Splits entries into contiguous groups that each stay under the per-output
/// byte budget, sized against the JSON text an entry renders to.
fn chunk_registration_entries(entries: &[RegistrationEntry]) -> Result<Vec<&[RegistrationEntry]>> {
    if entries.is_empty() {
        return Ok(vec![&[]]);
    }

    let mut chunks = Vec::new();
    let mut start = 0usize;
    let mut current_len = 2usize;

    for (index, entry) in entries.iter().enumerate() {
        let item_len = serde_json::to_string(&entry.entry)
            .map_err(|error| {
                AlienError::new(ErrorData::JsonSerializationFailed {
                    reason: format!(
                        "failed to estimate CloudFormation Outputs resource chunk size: {error}"
                    ),
                })
            })?
            .len();
        let is_first_in_chunk = index == start;
        let separator_len = usize::from(!is_first_in_chunk);
        if !is_first_in_chunk
            && current_len + separator_len + item_len > OUTPUT_RESOURCES_CHUNK_BYTES
        {
            chunks.push(&entries[start..index]);
            start = index;
            current_len = 2 + item_len;
            continue;
        }

        current_len += separator_len + item_len;
    }

    chunks.push(&entries[start..]);

    Ok(chunks)
}

pub(super) fn stack_settings_expression(
    target: CloudFormationTarget,
    stack: &Stack,
    settings: &StackSettings,
    kubernetes_namespace: Option<CfExpression>,
    supports_custom_domain: bool,
) -> CfExpression {
    let mut values = vec![
        ("deploymentModel", CfExpression::from("push")),
        ("updates", CfExpression::ref_(PARAM_UPDATES_MODE)),
        ("telemetry", CfExpression::ref_(PARAM_TELEMETRY_MODE)),
        ("heartbeats", CfExpression::ref_(PARAM_HEARTBEATS_MODE)),
        ("network", network_expression(settings.network.as_ref())),
    ];
    if supports_custom_domain {
        values.push(("domains", domains_expression()));
    }
    if target.is_kubernetes() {
        values.push((
            "kubernetes",
            kubernetes_settings_expression(settings.kubernetes.as_ref(), kubernetes_namespace),
        ));
    }
    if let Some(compute) = compute_settings_expression(stack, settings.compute.as_ref()) {
        values.push(("compute", compute));
    }
    CfExpression::object(values)
}
