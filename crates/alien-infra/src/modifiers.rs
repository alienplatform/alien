//! The template modifiers of a stack's extensions, which change what initial setup creates.
//!
//! After a generator, the modifiers run in the order the stack lists its extensions, and within
//! one extension in the order it registered them. Each gets what the previous one returned, so a
//! later extension can change a value an earlier one set.
//!
//! After every modifier, Alien refuses a template that removes, hides or retypes a resource,
//! naming the extension, so the template a customer's security team reviews shows every
//! resource Alien emits. Hiding is anything that keeps a resource in the text but out of the
//! deployment, or pulls in content the review doesn't see.

use crate::extensions::ExtensionHooks;
use crate::refusal::RefusalContext;
use alien_cloudformation::CfTemplate;
use alien_core::hooks::{HelmModifierOutput, HookKind, TemplateFiles, TerraformModifierOutput};
use alien_core::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_helm::HelmChart;
use alien_hooks_runtime::Limits;
use alien_terraform::ModuleFiles;
use indexmap::IndexMap;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

/// The template modifiers of a stack's extensions, in the order the stack lists them.
pub struct Modifiers {
    extensions: Vec<Arc<ExtensionHooks>>,
}

/// What one extension's modifiers changed in a template, by the names the template gives its
/// parts: resource logical ids and `<Section>.<name>` in CloudFormation, files and top-level
/// blocks in Terraform, file paths in a Helm chart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionChanges {
    pub extension: String,
    pub added: Vec<String>,
    pub changed: Vec<String>,
}

impl fmt::Display for ExtensionChanges {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.extension)?;
        let parts: Vec<String> = [("added", &self.added), ("changed", &self.changed)]
            .into_iter()
            .filter(|(_, names)| !names.is_empty())
            .map(|(verb, names)| format!("{verb} {}", names.join(", ")))
            .collect();
        if parts.is_empty() {
            write!(f, "no changes")
        } else {
            write!(f, "{}", parts.join("; "))
        }
    }
}

/// The changes that changed something, one line each, for a package's reviewers. A line lands in
/// a YAML comment and a Markdown list, so a control or format character, or a line or paragraph
/// separator, in a name is escaped: it can't start a line of its own or reorder the text around
/// it.
pub fn changes_for_review(changes: &[ExtensionChanges]) -> Vec<String> {
    changes
        .iter()
        .filter(|change| !change.added.is_empty() || !change.changed.is_empty())
        .map(|change| {
            change
                .to_string()
                .chars()
                .map(|c| {
                    if c.is_control() || is_format_or_separator(c) {
                        c.escape_default().to_string()
                    } else {
                        c.to_string()
                    }
                })
                .collect()
        })
        .collect()
}

/// The Unicode format characters (general category Cf, as of Unicode 15.1): bidirectional
/// overrides, zero-width characters and the like.
const FORMAT_CHARACTERS: [(char, char); 21] = [
    ('\u{00AD}', '\u{00AD}'),
    ('\u{0600}', '\u{0605}'),
    ('\u{061C}', '\u{061C}'),
    ('\u{06DD}', '\u{06DD}'),
    ('\u{070F}', '\u{070F}'),
    ('\u{0890}', '\u{0891}'),
    ('\u{08E2}', '\u{08E2}'),
    ('\u{180E}', '\u{180E}'),
    ('\u{200B}', '\u{200F}'),
    ('\u{202A}', '\u{202E}'),
    ('\u{2060}', '\u{2064}'),
    ('\u{2066}', '\u{206F}'),
    ('\u{FEFF}', '\u{FEFF}'),
    ('\u{FFF9}', '\u{FFFB}'),
    ('\u{110BD}', '\u{110BD}'),
    ('\u{110CD}', '\u{110CD}'),
    ('\u{13430}', '\u{1343F}'),
    ('\u{1BCA0}', '\u{1BCA3}'),
    ('\u{1D173}', '\u{1D17A}'),
    ('\u{E0001}', '\u{E0001}'),
    ('\u{E0020}', '\u{E007F}'),
];

fn is_format_or_separator(c: char) -> bool {
    matches!(c, '\u{2028}' | '\u{2029}')
        || FORMAT_CHARACTERS
            .iter()
            .any(|(first, last)| (*first..=*last).contains(&c))
}

/// Ends `module`'s README with what each extension changed in it, so whoever reviews the module
/// sees which of its parts an extension wrote. A module no extension changed is left as it is.
pub fn record_terraform_changes(module: &mut ModuleFiles, changes: &[ExtensionChanges]) {
    let lines = changes_for_review(changes);
    if lines.is_empty() {
        return;
    }
    let readme = module.files.entry("README.md".to_string()).or_default();
    if !readme.is_empty() && !readme.ends_with('\n') {
        readme.push('\n');
    }
    readme.push_str("\n## Changed by extensions\n\n");
    for line in lines {
        readme.push_str(&format!("- {line}\n"));
    }
}

#[derive(Debug, Clone, Copy)]
enum Format {
    CloudFormation,
    Terraform,
    Helm,
}

impl Format {
    fn kind(self) -> HookKind {
        match self {
            Format::CloudFormation => HookKind::CloudformationModifier,
            Format::Terraform => HookKind::TerraformModifier,
            Format::Helm => HookKind::HelmModifier,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Format::CloudFormation => "CloudFormation",
            Format::Terraform => "Terraform",
            Format::Helm => "Helm",
        }
    }
}

/// One modifier of one extension, as errors name it.
struct Modifier<'a> {
    hooks: &'a ExtensionHooks,
    hook: &'a str,
    format: Format,
}

impl Modifier<'_> {
    fn describe(&self) -> String {
        format!(
            "the {} modifier `{}` of extension '{}'",
            self.format.name(),
            self.hook,
            self.hooks.extension
        )
    }

    /// Runs the modifier in the extension's module on `input`, and reads its output.
    fn call<T: DeserializeOwned>(&self, input: &impl Serialize) -> Result<T> {
        let what = self.describe();
        let module = self.hooks.module(&what)?;
        let input = serde_json::to_vec(input).into_alien_error().context(
            ErrorData::JsonSerializationFailed {
                reason: format!("the input of {what} did not serialize"),
            },
        )?;
        let output = module
            .call(self.hook, &input, &Limits::for_kind(self.format.kind()))
            .context_as_wrapped(ErrorData::GenericError {
                message: format!("{what} failed"),
            })?;
        serde_json::from_slice(&output).into_alien_error().context(
            ErrorData::JsonDeserializationFailed {
                reason: format!("the output of {what} did not read"),
            },
        )
    }

    fn refuse(&self, what_it_did: String) -> AlienError<ErrorData> {
        crate::refusal::refusal(ErrorData::GenericError {
            message: format!("{} {what_it_did}", self.describe()),
        })
    }
}

fn unreadable(format: Format, reason: String) -> AlienError<ErrorData> {
    crate::refusal::refusal(ErrorData::GenericError {
        message: format!("the {} template does not read: {reason}", format.name()),
    })
}

impl Modifiers {
    pub(crate) fn new(extensions: Vec<Arc<ExtensionHooks>>) -> Self {
        Self { extensions }
    }

    /// Runs the CloudFormation modifiers on `template`. A modifier may add resources and change
    /// their properties. One that removes or retypes a resource, changes the `Condition`,
    /// `DeletionPolicy` or `UpdateReplacePolicy` of one of Alien's, or adds a `Transform`, an
    /// `Fn::Transform` or a nested stack fails the call.
    pub fn modify_cloudformation(
        &self,
        template: &mut CfTemplate,
    ) -> Result<Vec<ExtensionChanges>> {
        if self.extensions.is_empty() {
            return Ok(Vec::new());
        }
        let alien = template.clone();
        self.run(
            Format::CloudFormation,
            template,
            cloudformation_parts,
            |_, _| Ok(()),
            |modifier, template| {
                let mut modified: CfTemplate = modifier.call(&*template)?;
                // A logical id is the resource's key, not part of its JSON.
                for (logical_id, resource) in &mut modified.resources {
                    resource.logical_id = logical_id.clone();
                }
                check_cloudformation(&alien, template, &modified)
                    .map_err(|what| modifier.refuse(what))?;
                *template = modified;
                Ok(())
            },
        )
    }

    /// Runs the Terraform modifiers on `module`. A modifier returns the `.tf` files it changed,
    /// which replace the module's own; a new file may only be `<extension>.tf`. One that loses
    /// an address, declares one twice, or moves one of Alien's blocks or changes its `count`,
    /// `for_each`, `provider` or `lifecycle` fails the call.
    pub fn modify_terraform(&self, module: &mut ModuleFiles) -> Result<Vec<ExtensionChanges>> {
        if self.extensions.is_empty() {
            return Ok(Vec::new());
        }
        let alien = AlienModule::read(&module.files)
            .map_err(|reason| unreadable(Format::Terraform, reason))?;
        self.run(
            Format::Terraform,
            module,
            |module| terraform_parts(&module.files),
            |hooks, module| {
                let own_file = format!("{}.tf", hooks.extension);
                if module.files.contains_key(&own_file) {
                    return Err(format!(
                        "the module already has {own_file}, the file extension '{}' adds its resources to",
                        hooks.extension
                    ));
                }
                Ok(())
            },
            |modifier, module| {
                let output: TerraformModifierOutput = modifier.call(&TemplateFiles {
                    files: module.files.clone(),
                })?;
                module.files = modified_terraform(
                    &modifier.hooks.extension,
                    &alien,
                    &module.files,
                    output.changed,
                )
                .map_err(|what| modifier.refuse(what))?;
                Ok(())
            },
        )
    }

    /// Runs the Helm modifiers on `chart`. A modifier only adds templates, which land in the
    /// extension's own subchart, `charts/<extension>/`, whose values are its own. A name that
    /// isn't a plain template file name, one the chart has in any case, a template that defines
    /// a named template outside the `<extension>.` prefix, or one that changes a map in place
    /// or runs text as a template fails the call; so does an extension named like one of
    /// Alien's values keys or named templates.
    pub fn modify_helm(&self, chart: &mut HelmChart) -> Result<Vec<ExtensionChanges>> {
        if self.extensions.is_empty() {
            return Ok(Vec::new());
        }
        self.run(
            Format::Helm,
            chart,
            |chart| Ok(chart.files.clone()),
            |hooks, chart| {
                helm_extension_name(&hooks.extension, &chart.files)?;
                let subchart = alien_helm::extension_subchart(&hooks.extension);
                match chart
                    .files
                    .keys()
                    .find(|path| path.to_lowercase().starts_with(&subchart.to_lowercase()))
                {
                    Some(path) => Err(format!(
                        "the chart already has {path}, in the subchart extension '{}' adds its templates to",
                        hooks.extension
                    )),
                    None => Ok(()),
                }
            },
            |modifier, chart| {
                let output: HelmModifierOutput = modifier.call(&TemplateFiles {
                    files: chart.files.clone(),
                })?;
                let added =
                    helm_templates(&modifier.hooks.extension, &chart.files, output.templates)
                        .map_err(|what| modifier.refuse(what))?;
                chart.files.extend(added);
                Ok(())
            },
        )
    }

    /// Runs every modifier of `format` on `template`, extension by extension. `start` refuses a
    /// template an extension can't begin on, and `parts` names what the listing compares.
    fn run<T>(
        &self,
        format: Format,
        template: &mut T,
        parts: impl Fn(&T) -> Result<IndexMap<String, String>>,
        start: impl Fn(&ExtensionHooks, &T) -> std::result::Result<(), String>,
        modify: impl Fn(&Modifier<'_>, &mut T) -> Result<()>,
    ) -> Result<Vec<ExtensionChanges>> {
        let mut changes = Vec::with_capacity(self.extensions.len());
        for hooks in &self.extensions {
            let module = hooks.module(&format!("extension '{}'", hooks.extension))?;
            let before = parts(template)?;
            let mut modifiers = module
                .manifest
                .hooks
                .iter()
                .filter(|entry| entry.kind == format.kind())
                .peekable();
            if modifiers.peek().is_some() {
                start(hooks, template).map_err(|reason| {
                    crate::refusal::refusal(ErrorData::GenericError {
                        message: format!(
                            "the {} modifiers of extension '{}' can't run: {reason}",
                            format.name(),
                            hooks.extension
                        ),
                    })
                })?;
            }
            for entry in modifiers {
                modify(
                    &Modifier {
                        hooks,
                        hook: &entry.name,
                        format,
                    },
                    template,
                )?;
            }
            let after = parts(template)?;
            changes.push(ExtensionChanges {
                extension: hooks.extension.clone(),
                added: after
                    .keys()
                    .filter(|name| !before.contains_key(*name))
                    .cloned()
                    .collect(),
                changed: before
                    .iter()
                    .filter(|(name, part)| after.get(*name) != Some(*part))
                    .map(|(name, _)| name.clone())
                    .collect(),
            });
        }
        Ok(changes)
    }
}

/// Every part of a CloudFormation template, by name: each resource by its logical id, each
/// entry of the other sections as `<Section>.<name>`, and a section that is a single value,
/// such as `Transform`, by its name.
fn cloudformation_parts(template: &CfTemplate) -> Result<IndexMap<String, String>> {
    let Value::Object(sections) = serde_json::to_value(template).into_alien_error().context(
        ErrorData::JsonSerializationFailed {
            reason: "a CloudFormation template did not serialize".to_string(),
        },
    )?
    else {
        return Err(unreadable(
            Format::CloudFormation,
            "it is not a JSON object".to_string(),
        ));
    };
    let mut parts = IndexMap::new();
    for (section, value) in sections {
        match value {
            Value::Object(entries) => {
                for (name, entry) in entries {
                    let key = match section.as_str() {
                        "Resources" => name,
                        _ => format!("{section}.{name}"),
                    };
                    parts.insert(key, entry.to_string());
                }
            }
            value => {
                parts.insert(section, value.to_string());
            }
        }
    }
    Ok(parts)
}

/// `modified` keeps every resource of `before` with its type, and hides none of `alien`'s
/// (Alien's template, before any extension ran).
fn check_cloudformation(
    alien: &CfTemplate,
    before: &CfTemplate,
    modified: &CfTemplate,
) -> std::result::Result<(), String> {
    // The names a modifier adds are printed into the package's list of changes, a YAML comment
    // and a README line. CloudFormation's rule for logical names keeps each to one plain word.
    fn added<'a, V>(
        before: &'a IndexMap<String, V>,
        modified: &'a IndexMap<String, V>,
    ) -> impl Iterator<Item = &'a String> {
        modified.keys().filter(|name| !before.contains_key(*name))
    }
    for (section, mut names) in [
        (
            "Resources",
            Box::new(added(&before.resources, &modified.resources))
                as Box<dyn Iterator<Item = &String>>,
        ),
        (
            "Parameters",
            Box::new(added(&before.parameters, &modified.parameters)),
        ),
        (
            "Conditions",
            Box::new(added(&before.conditions, &modified.conditions)),
        ),
        (
            "Mappings",
            Box::new(added(&before.mappings, &modified.mappings)),
        ),
        (
            "Outputs",
            Box::new(added(&before.outputs, &modified.outputs)),
        ),
        ("Rules", Box::new(added(&before.rules, &modified.rules))),
        (
            "Metadata",
            Box::new(added(&before.metadata, &modified.metadata)),
        ),
    ] {
        if let Some(name) = names
            .find(|name| name.is_empty() || !name.bytes().all(|byte| byte.is_ascii_alphanumeric()))
        {
            return Err(format!(
                "added {section} entry {name:?}; a name it adds must be letters and digits only"
            ));
        }
    }
    for (logical_id, resource) in &before.resources {
        match modified.resources.get(logical_id) {
            None => {
                return Err(format!(
                    "removed {logical_id} ({}) from the template",
                    resource.resource_type
                ))
            }
            Some(kept) if kept.resource_type != resource.resource_type => {
                return Err(format!(
                    "changed the type of {logical_id} from {} to {}",
                    resource.resource_type, kept.resource_type
                ))
            }
            Some(_) => {}
        }
    }
    // A condition can keep a resource out of the stack, and a deletion or replacement policy
    // decides what happens to its data.
    for (logical_id, resource) in &alien.resources {
        let Some(kept) = modified.resources.get(logical_id) else {
            continue;
        };
        for (attribute, alien_value, value) in [
            ("Condition", &resource.condition, &kept.condition),
            (
                "DeletionPolicy",
                &resource.deletion_policy,
                &kept.deletion_policy,
            ),
            (
                "UpdateReplacePolicy",
                &resource.update_replace_policy,
                &kept.update_replace_policy,
            ),
        ] {
            if alien_value != value {
                return Err(format!(
                    "changed the {attribute} of {logical_id}, a resource Alien emits"
                ));
            }
        }
    }
    // Conditions decide whether Alien's resources exist, parameters and mappings feed them and
    // Alien's properties, outputs carry what Alien registers from the setup, rules check the
    // installer's parameters, and metadata labels them in the console the installer reads.
    for (section, alien_entries, entries) in [
        (
            "Rules",
            serde_json::to_value(&alien.rules),
            serde_json::to_value(&modified.rules),
        ),
        (
            "Metadata",
            serde_json::to_value(&alien.metadata),
            serde_json::to_value(&modified.metadata),
        ),
        (
            "Conditions",
            serde_json::to_value(&alien.conditions),
            serde_json::to_value(&modified.conditions),
        ),
        (
            "Parameters",
            serde_json::to_value(&alien.parameters),
            serde_json::to_value(&modified.parameters),
        ),
        (
            "Mappings",
            serde_json::to_value(&alien.mappings),
            serde_json::to_value(&modified.mappings),
        ),
        (
            "Outputs",
            serde_json::to_value(&alien.outputs),
            serde_json::to_value(&modified.outputs),
        ),
    ] {
        let (alien_entries, entries) = (
            alien_entries.map_err(|error| error.to_string())?,
            entries.map_err(|error| error.to_string())?,
        );
        if let Some((name, _)) = alien_entries
            .as_object()
            .into_iter()
            .flatten()
            .find(|(name, entry)| entries.get(name.as_str()) != Some(*entry))
        {
            let reader = match section {
                "Outputs" => "Alien registers",
                _ => "Alien's template reads",
            };
            return Err(format!(
                "changed or removed {section}.{name}, which {reader}"
            ));
        }
    }
    // A transform or a nested stack runs content from elsewhere when the stack deploys, which
    // the template under review doesn't show.
    if modified.transform != alien.transform {
        return Err("changed the template's Transform".to_string());
    }
    let mut allowed = fn_transforms(alien)?;
    for transform in fn_transforms(modified)? {
        match allowed.iter().position(|alien| *alien == transform) {
            Some(index) => {
                allowed.swap_remove(index);
            }
            None => return Err(format!("added an Fn::Transform: {transform}")),
        }
    }
    for (logical_id, resource) in &modified.resources {
        if matches!(
            resource.resource_type.as_str(),
            "AWS::CloudFormation::Stack" | "AWS::CloudFormation::StackSet"
        ) && alien.resources.get(logical_id) != Some(resource)
        {
            return Err(format!(
                "added or changed {logical_id}, a nested stack ({})",
                resource.resource_type
            ));
        }
    }
    // A reference to something the template doesn't define fails the stack when CloudFormation
    // reads it, after the customer started setup; only one the modifier added counts.
    let dangling = dangling_references(modified)?;
    let already = dangling_references(before)?;
    if let Some(name) = dangling.iter().find(|name| !already.contains(*name)) {
        return Err(format!(
            "refers to {name}, which the template doesn't define"
        ));
    }
    // The installer reads the description in the console before creating the stack.
    if modified.description != alien.description {
        return Err("changed the template's Description".to_string());
    }
    Ok(())
}

/// The CloudFormation pseudo parameters, which a template refers to without defining.
const PSEUDO_PARAMETERS: [&str; 8] = [
    "AWS::AccountId",
    "AWS::NotificationARNs",
    "AWS::NoValue",
    "AWS::Partition",
    "AWS::Region",
    "AWS::StackId",
    "AWS::StackName",
    "AWS::URLSuffix",
];

/// What `template` refers to that it doesn't define: a `Ref` or `Fn::Sub` name that is neither
/// a resource, a parameter nor a pseudo parameter; a `Fn::GetAtt`, `Fn::Sub` attribute or
/// `DependsOn` name that is not a resource; a condition a resource, an `Fn::If` or another
/// condition names; and a mapping `Fn::FindInMap` reads.
fn dangling_references(template: &CfTemplate) -> std::result::Result<Vec<String>, String> {
    #[derive(Default)]
    struct Found {
        /// `Ref` and `Fn::Sub` names: a resource, a parameter or a pseudo parameter
        names: Vec<String>,
        /// `Fn::GetAtt`, `Fn::Sub` attribute and `DependsOn` names: a resource
        resources: Vec<String>,
        conditions: Vec<String>,
        /// A mapping's name, and its top-level and second-level keys where they are literals.
        mappings: Vec<(String, Option<String>, Option<String>)>,
    }
    fn subs(text: &str, own: &[String], found: &mut Found) {
        let mut rest = text;
        while let Some(start) = rest.find("${") {
            rest = &rest[start + 2..];
            let Some(end) = rest.find('}') else { break };
            let name = &rest[..end];
            rest = &rest[end + 1..];
            // `${!Literal}` is text, not a reference.
            if name.starts_with('!') {
                continue;
            }
            match name.split_once('.') {
                _ if own
                    .iter()
                    .any(|own| own == name.split('.').next().unwrap_or(name)) => {}
                Some((resource, _)) => found.resources.push(resource.to_string()),
                None => found.names.push(name.to_string()),
            }
        }
    }
    /// `in_conditions` is set inside the Conditions section, where `{"Condition": name}` names
    /// another condition; elsewhere a `Condition` key can be a resource property.
    fn collect(value: &Value, in_conditions: bool, found: &mut Found) {
        match value {
            Value::Object(entries) => {
                for (key, entry) in entries {
                    match (key.as_str(), entry) {
                        ("Ref", Value::String(name)) => found.names.push(name.clone()),
                        ("Fn::GetAtt", Value::Array(parts)) => {
                            if let Some(Value::String(name)) = parts.first() {
                                found.resources.push(name.clone());
                            }
                        }
                        ("Fn::GetAtt", Value::String(path)) => {
                            found
                                .resources
                                .push(path.split('.').next().unwrap_or(path).to_string());
                        }
                        ("Fn::Sub", Value::String(text)) => subs(text, &[], found),
                        ("Fn::Sub", Value::Array(parts)) => {
                            // Names the second element defines are the Sub's own.
                            let own: Vec<String> = parts
                                .get(1)
                                .and_then(Value::as_object)
                                .map(|names| names.keys().cloned().collect())
                                .unwrap_or_default();
                            if let Some(Value::String(text)) = parts.first() {
                                subs(text, &own, found);
                            }
                        }
                        ("Fn::If", Value::Array(parts)) => {
                            if let Some(Value::String(name)) = parts.first() {
                                found.conditions.push(name.clone());
                            }
                        }
                        ("Fn::FindInMap", Value::Array(parts)) => {
                            if let Some(Value::String(name)) = parts.first() {
                                let literal = |index: usize| match parts.get(index) {
                                    Some(Value::String(key)) => Some(key.clone()),
                                    _ => None,
                                };
                                let top = literal(1);
                                let second = top.as_ref().and(literal(2));
                                found.mappings.push((name.clone(), top, second));
                            }
                        }
                        ("Condition", Value::String(name)) if in_conditions => {
                            found.conditions.push(name.clone());
                        }
                        _ => {}
                    }
                    collect(entry, in_conditions, found);
                }
            }
            Value::Array(items) => items
                .iter()
                .for_each(|item| collect(item, in_conditions, found)),
            _ => {}
        }
    }
    let value = serde_json::to_value(template).map_err(|error| error.to_string())?;
    let mut found = Found::default();
    for (section, entries) in value.as_object().into_iter().flatten() {
        collect(entries, section == "Conditions", &mut found);
    }
    for resource in template.resources.values() {
        found.resources.extend(resource.depends_on.iter().cloned());
        found.conditions.extend(resource.condition.iter().cloned());
    }
    let is_resource = |name: &String| template.resources.contains_key(name);
    let mut dangling: Vec<String> = found
        .names
        .into_iter()
        .filter(|name| {
            if name.starts_with("AWS::") {
                !PSEUDO_PARAMETERS.contains(&name.as_str())
            } else {
                !is_resource(name) && !template.parameters.contains_key(name)
            }
        })
        .chain(
            found
                .resources
                .into_iter()
                .filter(|name| !is_resource(name))
                .map(|name| match template.parameters.contains_key(&name) {
                    true => format!("{name} as a resource"),
                    false => name,
                }),
        )
        .chain(
            found
                .conditions
                .into_iter()
                .filter(|name| !template.conditions.contains_key(name))
                .map(|name| format!("condition {name}")),
        )
        .chain(
            found
                .mappings
                .into_iter()
                .filter_map(|(name, top, second)| {
                    let Some(mapping) = template.mappings.get(&name) else {
                        return Some(format!("mapping {name}"));
                    };
                    let top = top?;
                    let Some(keys) = mapping.get(&top) else {
                        return Some(format!("mapping {name}.{top}"));
                    };
                    let second = second?;
                    (!keys.contains_key(&second)).then(|| format!("mapping {name}.{top}.{second}"))
                }),
        )
        .collect();
    dangling.sort();
    dangling.dedup();
    Ok(dangling)
}

/// Every `Fn::Transform` in the template, as JSON text.
fn fn_transforms(template: &CfTemplate) -> std::result::Result<Vec<String>, String> {
    fn collect(value: &Value, found: &mut Vec<String>) {
        match value {
            Value::Object(entries) => {
                for (key, entry) in entries {
                    if key == "Fn::Transform" {
                        found.push(entry.to_string());
                    }
                    collect(entry, found);
                }
            }
            Value::Array(items) => items.iter().for_each(|item| collect(item, found)),
            _ => {}
        }
    }
    let value = serde_json::to_value(template).map_err(|error| error.to_string())?;
    let mut found = Vec::new();
    collect(&value, &mut found);
    Ok(found)
}

/// A resource, data source or ephemeral resource the module declares: the file it is in, and
/// its block.
type Declared = IndexMap<String, (String, hcl::Block)>;

/// Alien's module, before any extension ran, as the Terraform checks compare with it.
struct AlienModule {
    declared: Declared,
    /// Each of Alien's `.tf` files with its top-level blocks other than resources: variables,
    /// locals, providers, `terraform`, data sources, modules, outputs and the rest. They decide
    /// whether and where Alien's resources are created, and outputs carry what Alien registers
    /// from the setup. Every `count` and `for_each` Alien emits
    /// reads only variables, locals, data sources and the instances of Alien's resources, whose
    /// own `count` and `for_each` are frozen too, so freezing these blocks leaves an extension no
    /// way to change how many of Alien's resources exist.
    settings: IndexMap<String, Vec<hcl::Block>>,
}

impl AlienModule {
    fn read(files: &IndexMap<String, String>) -> std::result::Result<Self, String> {
        Ok(Self {
            declared: terraform_addresses(files)?,
            settings: settings_blocks(files)?,
        })
    }
}

/// The top-level blocks of each `.tf` file that aren't resources.
fn settings_blocks(
    files: &IndexMap<String, String>,
) -> std::result::Result<IndexMap<String, Vec<hcl::Block>>, String> {
    files
        .iter()
        .filter(|(path, _)| path.ends_with(".tf"))
        .map(|(path, contents)| {
            let body =
                hcl::parse(contents).map_err(|error| format!("{path} is not HCL: {error}"))?;
            let blocks = body
                .into_blocks()
                .filter(|block| block.identifier.as_str() != "resource")
                .collect();
            Ok((path.clone(), blocks))
        })
        .collect()
}

/// A block's type and labels, as its first line shows them.
fn block_name(block: &hcl::Block) -> String {
    std::iter::once(block.identifier.as_str())
        .chain(block.labels.iter().map(|label| label.as_str()))
        .collect::<Vec<_>>()
        .join(".")
}

/// The blocks an extension's file may not declare: they configure Terraform or a provider, or
/// move, adopt or forget resources, which reaches Alien's resources from outside their blocks.
const EXTENSION_FILE_REFUSED: [&str; 5] = ["terraform", "provider", "moved", "removed", "import"];

/// Refuses an extension's file that declares what [`EXTENSION_FILE_REFUSED`] lists, a module
/// from anywhere but a local path, whose contents the review wouldn't see, or a block with a
/// label that isn't an identifier.
fn check_extension_file(path: &str, contents: &str) -> std::result::Result<(), String> {
    let body = hcl::parse(contents).map_err(|error| format!("{path} is not HCL: {error}"))?;
    for block in body.blocks() {
        check_labels(path, block)?;
        let identifier = block.identifier.as_str();
        if EXTENSION_FILE_REFUSED.contains(&identifier) {
            return Err(format!(
                "declares {} {identifier} block in {path}; an extension's file may declare resources, data sources, variables, locals, outputs and local modules",
                if identifier == "import" { "an" } else { "a" }
            ));
        }
        if identifier == "module" {
            let local = block
                .body
                .attributes()
                .find(|attribute| attribute.key.as_str() == "source")
                .is_some_and(|source| {
                    matches!(&source.expr, hcl::Expression::String(source) if source.starts_with("./") || source.starts_with("../"))
                });
            if !local {
                return Err(format!(
                    "declares {} in {path} with a source that isn't a local path",
                    block_name(block)
                ));
            }
        }
    }
    Ok(())
}

/// Refuses a block with a label that isn't a Terraform identifier: a letter or `_`, then letters,
/// digits, `_` and `-`. A label lands in the README's list of changes, where any other character
/// could leave its line or format it.
fn check_labels(path: &str, block: &hcl::Block) -> std::result::Result<(), String> {
    let identifier = |label: &str| {
        let mut chars = label.chars();
        chars
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    };
    match block
        .labels
        .iter()
        .map(|label| label.as_str())
        .find(|label| !identifier(label))
    {
        Some(label) => Err(format!(
            "declares a {} block in {path} labeled {label:?}, which isn't a Terraform identifier",
            block.identifier.as_str()
        )),
        None => Ok(()),
    }
}

/// The module `extension`'s modifier made of `before` by changing the files in `changed`. Each
/// is one of the module's `.tf` files or `<extension>.tf`. The result declares every address
/// `before` declares and none twice, keeps each of `alien`'s blocks in its file with its
/// meta-arguments, and keeps every block of Alien's files other than resources as Alien wrote
/// it. An address names its type, so a changed type is a lost address.
fn modified_terraform(
    extension: &str,
    alien: &AlienModule,
    before: &IndexMap<String, String>,
    changed: IndexMap<String, String>,
) -> std::result::Result<IndexMap<String, String>, String> {
    let own_file = format!("{extension}.tf");
    if let Some(path) = changed
        .keys()
        .find(|path| !path.ends_with(".tf") || (!before.contains_key(*path) && **path != own_file))
    {
        return Err(format!(
            "wrote {path}; it may change the module's .tf files and add {own_file}"
        ));
    }
    for (path, contents) in &changed {
        if !alien.settings.contains_key(path) {
            check_extension_file(path, contents)?;
        }
    }
    let mut after = before.clone();
    after.extend(changed);
    let declared = terraform_addresses(&after)?;
    let declared_before = terraform_addresses(before)?;
    if let Some(address) = declared_before
        .keys()
        .find(|address| !declared.contains_key(*address))
    {
        return Err(format!("removed {address} from the module"));
    }
    for (address, (path, block)) in &declared {
        if !declared_before.contains_key(address) {
            check_labels(path, block)?;
        }
    }
    let settings = settings_blocks(&after)?;
    for (path, blocks) in &alien.settings {
        let kept = &settings[path];
        if let Some(block) = blocks.iter().find(|block| !kept.contains(block)) {
            let reader = match block.identifier.as_str() {
                "output" => "Alien registers",
                _ => "Alien's resources read",
            };
            return Err(format!(
                "changed or removed {} in {path}, which {reader}",
                block_name(block)
            ));
        }
        if let Some(block) = kept.iter().find(|block| !blocks.contains(block)) {
            return Err(format!(
                "added {} to {path}, one of Alien's files; an extension declares its own in {own_file}",
                block_name(block)
            ));
        }
    }
    for (address, (path, block)) in &alien.declared {
        let (kept_path, kept) = &declared[address];
        if kept_path != path {
            return Err(format!(
                "moved {address}, a block Alien emits, from {path} to {kept_path}"
            ));
        }
        if let Some(meta) = changed_meta_argument(block, kept) {
            return Err(format!(
                "changed the {meta} of {address}, a block Alien emits"
            ));
        }
    }
    let already = dangling_terraform_references(before)?;
    if let Some(name) = dangling_terraform_references(&after)?
        .into_iter()
        .find(|name| !already.contains(name))
    {
        return Err(format!(
            "refers to {name}, which the module doesn't declare"
        ));
    }
    Ok(after)
}

/// What the module's expressions refer to that it doesn't declare: a variable, local, module,
/// resource, data source or ephemeral resource, by the name an expression gives it, and an
/// aliased provider configuration a `provider` or `providers` argument names, as
/// `provider <name>.<alias>`.
fn dangling_terraform_references(
    files: &IndexMap<String, String>,
) -> std::result::Result<BTreeSet<String>, String> {
    let mut declared = BTreeSet::new();
    let mut referenced = BTreeSet::new();
    for (path, contents) in files.iter().filter(|(path, _)| path.ends_with(".tf")) {
        let body = hcl::parse(contents).map_err(|error| format!("{path} is not HCL: {error}"))?;
        for block in body.blocks() {
            let labels: Vec<&str> = block.labels.iter().map(|label| label.as_str()).collect();
            match (block.identifier.as_str(), labels.as_slice()) {
                ("variable", [name]) => {
                    declared.insert(format!("var.{name}"));
                }
                ("module", [name]) => {
                    declared.insert(format!("module.{name}"));
                }
                ("locals", []) => declared.extend(
                    block
                        .body
                        .attributes()
                        .map(|attribute| format!("local.{}", attribute.key.as_str())),
                ),
                // A check block's data sources are scoped to it, but named like any other.
                ("check", _) => declared.extend(block.body.blocks().filter_map(address)),
                ("provider", [name]) => {
                    if let Some(hcl::Expression::String(alias)) = block
                        .body
                        .attributes()
                        .find(|attribute| attribute.key.as_str() == "alias")
                        .map(|attribute| &attribute.expr)
                    {
                        declared.insert(format!("provider {name}.{alias}"));
                    }
                }
                ("terraform", []) => {
                    for required in block
                        .body
                        .blocks()
                        .filter(|block| block.identifier.as_str() == "required_providers")
                    {
                        for attribute in required.body.attributes() {
                            let hcl::Expression::Object(requirement) = &attribute.expr else {
                                continue;
                            };
                            for (key, value) in requirement {
                                if matches!(key, hcl::ObjectKey::Identifier(key) if key.as_str() == "configuration_aliases")
                                {
                                    if let hcl::Expression::Array(aliases) = value {
                                        declared
                                            .extend(aliases.iter().filter_map(provider_reference));
                                    }
                                }
                            }
                        }
                    }
                }
                _ => declared.extend(address(block)),
            }
        }
        body_references(&body, Body::File, &mut Vec::new(), &mut referenced);
    }
    Ok(referenced.difference(&declared).cloned().collect())
}

/// Where a body sits, which decides what its `provider` and `providers` arguments are.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Body {
    /// A `.tf` file's top level.
    File,
    /// A top-level `check` block's.
    Check,
    /// A resource's, data source's, ephemeral resource's or module's, or a check block's data
    /// source's: `provider` and `providers` are meta-arguments naming provider configurations.
    MetaArguments,
    /// Any other block's: `provider` and `providers` are ordinary arguments.
    Nested,
}

impl Body {
    fn of(self, block: &hcl::Block) -> Body {
        match (self, block.identifier.as_str()) {
            (Body::File, "resource" | "data" | "ephemeral" | "module") | (Body::Check, "data") => {
                Body::MetaArguments
            }
            (Body::File, "check") => Body::Check,
            _ => Body::Nested,
        }
    }
}

/// The aliased provider configuration `expression` names, such as `provider aws.west`. A
/// provider's default configuration, `aws`, needs no block, so it names none.
fn provider_reference(expression: &hcl::Expression) -> Option<String> {
    let hcl::Expression::Traversal(traversal) = expression else {
        return None;
    };
    let hcl::Expression::Variable(name) = &traversal.expr else {
        return None;
    };
    match traversal.operators.as_slice() {
        [hcl::TraversalOperator::GetAttr(alias)] => {
            Some(format!("provider {}.{}", name.as_str(), alias.as_str()))
        }
        _ => None,
    }
}

fn body_references(
    body: &hcl::Body,
    kind: Body,
    scope: &mut Vec<String>,
    found: &mut BTreeSet<String>,
) {
    let meta_arguments = kind == Body::MetaArguments;
    for structure in body.iter() {
        match structure {
            hcl::Structure::Attribute(attribute) => match attribute.key.as_str() {
                "provider" if meta_arguments => found.extend(provider_reference(&attribute.expr)),
                // The keys name the child module's configurations; the values are this one's.
                "providers" if meta_arguments => {
                    if let hcl::Expression::Object(providers) = &attribute.expr {
                        found.extend(providers.values().filter_map(provider_reference));
                    }
                }
                // An attribute path, not an object of the module.
                "ignore_changes" => {}
                _ => expression_references(&attribute.expr, scope, found),
            },
            // Declares provider configurations; `dangling_terraform_references` reads them.
            hcl::Structure::Block(block) if block.identifier.as_str() == "required_providers" => {}
            hcl::Structure::Block(block) if block.identifier.as_str() == "dynamic" => {
                let iterator = block
                    .body
                    .attributes()
                    .find(|attribute| attribute.key.as_str() == "iterator")
                    .and_then(|attribute| match &attribute.expr {
                        hcl::Expression::Variable(name) => Some(name.as_str().to_string()),
                        _ => None,
                    })
                    .or_else(|| block.labels.first().map(|label| label.as_str().to_string()));
                scope.extend(iterator.clone());
                body_references(&block.body, Body::Nested, scope, found);
                if iterator.is_some() {
                    scope.pop();
                }
            }
            hcl::Structure::Block(block) => {
                body_references(&block.body, kind.of(block), scope, found)
            }
        }
    }
}

fn expression_references(
    expression: &hcl::Expression,
    scope: &mut Vec<String>,
    found: &mut BTreeSet<String>,
) {
    use hcl::Expression;
    match expression {
        Expression::Array(items) => {
            for item in items {
                expression_references(item, scope, found);
            }
        }
        Expression::Object(object) => {
            for (key, value) in object {
                if let hcl::ObjectKey::Expression(key) = key {
                    expression_references(key, scope, found);
                }
                expression_references(value, scope, found);
            }
        }
        Expression::TemplateExpr(template) => {
            // Terraform refuses a template that doesn't parse, so there is nothing to check.
            if let Ok(template) = hcl::template::Template::from_expr(template) {
                template_references(&template, scope, found);
            }
        }
        Expression::Traversal(traversal) => {
            match &traversal.expr {
                Expression::Variable(root) => {
                    if let Some(name) = reference_name(root.as_str(), &traversal.operators, scope) {
                        found.insert(name);
                    }
                }
                other => expression_references(other, scope, found),
            }
            for operator in &traversal.operators {
                if let hcl::TraversalOperator::Index(index) = operator {
                    expression_references(index, scope, found);
                }
            }
        }
        Expression::FuncCall(call) => {
            for argument in &call.args {
                expression_references(argument, scope, found);
            }
        }
        Expression::Parenthesis(inner) => expression_references(inner, scope, found),
        Expression::Conditional(conditional) => {
            expression_references(&conditional.cond_expr, scope, found);
            expression_references(&conditional.true_expr, scope, found);
            expression_references(&conditional.false_expr, scope, found);
        }
        Expression::Operation(operation) => match operation.as_ref() {
            hcl::Operation::Unary(unary) => expression_references(&unary.expr, scope, found),
            hcl::Operation::Binary(binary) => {
                expression_references(&binary.lhs_expr, scope, found);
                expression_references(&binary.rhs_expr, scope, found);
            }
        },
        Expression::ForExpr(for_expr) => {
            expression_references(&for_expr.collection_expr, scope, found);
            let depth = scope.len();
            scope.extend(
                for_expr
                    .key_var
                    .iter()
                    .map(|name| name.as_str().to_string()),
            );
            scope.push(for_expr.value_var.as_str().to_string());
            let key = for_expr.key_expr.iter();
            let condition = for_expr.cond_expr.iter();
            for inner in key.chain([&for_expr.value_expr]).chain(condition) {
                expression_references(inner, scope, found);
            }
            scope.truncate(depth);
        }
        _ => {}
    }
}

fn template_references(
    template: &hcl::template::Template,
    scope: &mut Vec<String>,
    found: &mut BTreeSet<String>,
) {
    use hcl::template::{Directive, Element};
    for element in template.elements() {
        match element {
            Element::Literal(_) => {}
            Element::Interpolation(interpolation) => {
                expression_references(&interpolation.expr, scope, found)
            }
            Element::Directive(directive) => match directive.as_ref() {
                Directive::If(directive) => {
                    expression_references(&directive.cond_expr, scope, found);
                    template_references(&directive.true_template, scope, found);
                    if let Some(false_template) = &directive.false_template {
                        template_references(false_template, scope, found);
                    }
                }
                Directive::For(directive) => {
                    expression_references(&directive.collection_expr, scope, found);
                    let depth = scope.len();
                    scope.extend(
                        directive
                            .key_var
                            .iter()
                            .map(|name| name.as_str().to_string()),
                    );
                    scope.push(directive.value_var.as_str().to_string());
                    template_references(&directive.template, scope, found);
                    scope.truncate(depth);
                }
            },
        }
    }
}

/// The module object a traversal from `root` names, such as `var.region` or
/// `data.aws_iam_policy_document.read`, or `None` when it names none: Terraform's own values
/// (`path`, `terraform`, `count`, `each`, `self`) or an iteration variable in `scope`.
fn reference_name(
    root: &str,
    operators: &[hcl::TraversalOperator],
    scope: &[String],
) -> Option<String> {
    let parts = match root {
        "path" | "terraform" | "count" | "each" | "self" => return None,
        _ if scope.iter().any(|name| name == root) => return None,
        "data" | "ephemeral" => 2,
        _ => 1,
    };
    let attributes: Vec<&str> = operators
        .iter()
        .map_while(|operator| match operator {
            hcl::TraversalOperator::GetAttr(name) => Some(name.as_str()),
            _ => None,
        })
        .take(parts)
        .collect();
    (attributes.len() == parts).then(|| {
        std::iter::once(root)
            .chain(attributes)
            .collect::<Vec<_>>()
            .join(".")
    })
}

/// The first meta-argument that differs between two versions of a block: one that decides
/// whether, where or how often Terraform creates it, or keeps it from being changed.
fn changed_meta_argument(alien: &hcl::Block, modified: &hcl::Block) -> Option<&'static str> {
    let attribute = |block: &hcl::Block, name: &str| {
        block
            .body
            .attributes()
            .find(|attribute| attribute.key.as_str() == name)
            .map(|attribute| attribute.expr.clone())
    };
    let lifecycle = |block: &hcl::Block| {
        block
            .body
            .blocks()
            .filter(|nested| nested.identifier.as_str() == "lifecycle")
            .cloned()
            .collect::<Vec<_>>()
    };
    ["count", "for_each", "provider"]
        .into_iter()
        .find(|name| attribute(alien, name) != attribute(modified, name))
        .or_else(|| (lifecycle(alien) != lifecycle(modified)).then_some("lifecycle"))
}

/// The resources, data sources and ephemeral resources the module's `.tf` files declare, by
/// address. Terraform refuses a module that declares an address twice, so this does too.
fn terraform_addresses(files: &IndexMap<String, String>) -> std::result::Result<Declared, String> {
    let mut declared = Declared::new();
    for (path, contents) in files.iter().filter(|(path, _)| path.ends_with(".tf")) {
        let body = hcl::parse(contents).map_err(|error| format!("{path} is not HCL: {error}"))?;
        for block in body.into_blocks() {
            let Some(address) = address(&block) else {
                continue;
            };
            if let Some((first, _)) = declared.get(&address) {
                return Err(format!(
                    "declares {address} in {path}, which {first} declares"
                ));
            }
            declared.insert(address, (path.clone(), block));
        }
    }
    Ok(declared)
}

fn address(block: &hcl::Block) -> Option<String> {
    let labels: Vec<&str> = block.labels.iter().map(|label| label.as_str()).collect();
    match (block.identifier.as_str(), labels.as_slice()) {
        ("resource", [resource_type, name]) => Some(format!("{resource_type}.{name}")),
        ("data", [data_type, name]) => Some(format!("data.{data_type}.{name}")),
        ("ephemeral", [resource_type, name]) => Some(format!("ephemeral.{resource_type}.{name}")),
        _ => None,
    }
}

/// Every part of a Terraform module, by name: each file by its path, each resource, data
/// source and ephemeral resource by its address, and each other top-level block as
/// `<type>[.<label>...] in <file>`.
fn terraform_parts(files: &IndexMap<String, String>) -> Result<IndexMap<String, String>> {
    let mut parts = IndexMap::new();
    for (path, contents) in files {
        parts.insert(path.clone(), contents.clone());
        if !path.ends_with(".tf") {
            continue;
        }
        let body = hcl::parse(contents)
            .map_err(|error| unreadable(Format::Terraform, format!("{path}: {error}")))?;
        for block in body.into_blocks() {
            let name = address(&block).unwrap_or_else(|| {
                std::iter::once(block.identifier.as_str())
                    .chain(block.labels.iter().map(|label| label.as_str()))
                    .collect::<Vec<_>>()
                    .join(".")
                    + " in "
                    + path
            });
            let mut key = name.clone();
            let mut repeat = 1;
            while parts.contains_key(&key) {
                repeat += 1;
                key = format!("{name} #{repeat}");
            }
            parts.insert(key, format!("{block:?}"));
        }
    }
    Ok(parts)
}

/// The files `templates` add to a chart with `files`, in `extension`'s subchart. The first time,
/// the subchart's `Chart.yaml` and `values.yaml` too, and the chart's own `Chart.yaml` and
/// values schema with the subchart in them, since Helm refuses a subchart its parent doesn't
/// list and values its schema doesn't allow.
fn helm_templates(
    extension: &str,
    files: &IndexMap<String, String>,
    templates: BTreeMap<String, String>,
) -> std::result::Result<Vec<(String, String)>, String> {
    let subchart = alien_helm::extension_subchart(extension);
    let taken: Vec<String> = files.keys().map(|path| path.to_lowercase()).collect();
    let mut added = Vec::new();
    if !files.contains_key(&format!("{subchart}Chart.yaml")) {
        added.push((
            format!("{subchart}Chart.yaml"),
            format!(
                "apiVersion: v2\nname: {extension}\ndescription: The templates of the {extension} extension\ntype: application\nversion: 0.0.0\n"
            ),
        ));
        added.push((format!("{subchart}values.yaml"), "{}\n".to_string()));
        if let Some(chart) = files.get("Chart.yaml") {
            added.push(("Chart.yaml".to_string(), with_dependency(chart, extension)?));
        }
        if let Some(schema) = files.get("values.schema.json") {
            added.push((
                "values.schema.json".to_string(),
                with_subchart_values(schema, extension)?,
            ));
        }
    }
    for (name, contents) in templates {
        if !is_template_file_name(&name) {
            return Err(format!(
                "returned a template named {name:?}; a name is lowercase letters, digits, '.', '_' and '-', and ends in .yaml, .yml or .tpl"
            ));
        }
        let path = format!("{subchart}templates/{name}");
        if taken.contains(&path.to_lowercase()) {
            return Err(format!("returned {path}, which the chart already has"));
        }
        if let Some(function) = refused_function(&contents) {
            return Err(format!(
                "calls {function} in {name}; a template of an extension may not change a value in place or run text as a template, since Helm shares `.Release` and the chart's values with every subchart"
            ));
        }
        if let Some(defined) = defined_templates(&contents)
            .into_iter()
            .find(|defined| !defined.starts_with(&format!("{extension}.")))
        {
            return Err(format!(
                "defines the named template {defined:?} in {name}; an extension's named templates start with \"{extension}.\", since every template in a chart shares one namespace"
            ));
        }
        added.push((path, contents));
    }
    Ok(added)
}

/// The chart's `Chart.yaml` with `extension`'s subchart among its dependencies.
fn with_dependency(chart: &str, extension: &str) -> std::result::Result<String, String> {
    let mut chart: serde_yaml::Mapping = serde_yaml::from_str(chart)
        .map_err(|error| format!("left a Chart.yaml that does not read: {error}"))?;
    let dependencies = chart
        .entry("dependencies".into())
        .or_insert_with(|| serde_yaml::Value::Sequence(Vec::new()));
    let Some(dependencies) = dependencies.as_sequence_mut() else {
        return Err("left a Chart.yaml whose dependencies are not a list".to_string());
    };
    let mut dependency = serde_yaml::Mapping::new();
    dependency.insert("name".into(), extension.into());
    dependency.insert("version".into(), "0.0.0".into());
    dependencies.push(dependency.into());
    serde_yaml::to_string(&chart).map_err(|error| error.to_string())
}

/// The chart's values schema with the values of `extension`'s subchart allowed, added as text
/// so every other byte stays as Alien wrote it.
fn with_subchart_values(schema: &str, extension: &str) -> std::result::Result<String, String> {
    const PROPERTIES: &str = "\"properties\": {";
    let at = schema
        .find(PROPERTIES)
        .ok_or("left a values schema without properties")?
        + PROPERTIES.len();
    let modified = format!(
        "{}\n    \"{extension}\": {{ \"type\": \"object\" }},{}",
        &schema[..at],
        &schema[at..]
    );
    let parsed: Value = serde_json::from_str(&modified)
        .map_err(|error| format!("left a values schema that does not read: {error}"))?;
    match parsed["properties"].get(extension) {
        Some(_) => Ok(modified),
        None => Err("left a values schema whose first properties are not its own".to_string()),
    }
}

/// The prefix of the named templates Alien's chart defines, such as `deployment.fullname`.
const ALIEN_TEMPLATE_PREFIX: &str = "deployment";

/// Refuses an extension name a subchart can't take: one of the top-level keys of Alien's
/// values, which the subchart's values would share, or one starting with the prefix of Alien's
/// named templates, which the extension's own would share.
fn helm_extension_name(
    extension: &str,
    files: &IndexMap<String, String>,
) -> std::result::Result<(), String> {
    let name = extension.to_lowercase();
    if name.starts_with(ALIEN_TEMPLATE_PREFIX) {
        return Err(format!(
            "its name starts with \"{ALIEN_TEMPLATE_PREFIX}\", the prefix of the chart's named templates"
        ));
    }
    let values: serde_yaml::Mapping = match files.get("values.yaml") {
        Some(values) => serde_yaml::from_str(values)
            .map_err(|error| format!("the chart's values.yaml does not read: {error}"))?,
        None => serde_yaml::Mapping::new(),
    };
    match values
        .keys()
        .filter_map(serde_yaml::Value::as_str)
        .find(|key| key.to_lowercase() == name)
    {
        Some(key) => Err(format!(
            "its name is `{key}`, a key of the chart's values, which its subchart's values would replace"
        )),
        None => Ok(()),
    }
}

/// Template functions a subchart's template may not call: each changes a map in place or runs
/// text as a template, and Helm hands every subchart the same `.Release` and a part of the
/// chart's values, so either would change what Alien's templates render.
const REFUSED_FUNCTIONS: [&str; 7] = [
    "set",
    "unset",
    "merge",
    "mergeOverwrite",
    "mustMerge",
    "mustMergeOverwrite",
    "tpl",
];

/// The first of [`REFUSED_FUNCTIONS`] a template's actions name. Only the words inside `{{ }}`
/// count, outside strings and comments, and not a field (`.set`) or a variable (`$set`).
fn refused_function(contents: &str) -> Option<&'static str> {
    template_actions(contents)
        .into_iter()
        .flat_map(action_words)
        .find_map(|word| {
            REFUSED_FUNCTIONS
                .iter()
                .copied()
                .find(|refused| *refused == word)
        })
}

/// The text of each `{{ }}` action in a template, without its delimiters.
fn template_actions(contents: &str) -> Vec<&str> {
    let mut actions = Vec::new();
    let mut rest = contents;
    while let Some(open) = rest.find("{{") {
        let action = &rest[open + 2..];
        let mut quote = None;
        let mut escaped = false;
        let mut end = action.len();
        for (index, character) in action.char_indices() {
            match quote {
                Some('"') if escaped => escaped = false,
                Some('"') if character == '\\' => escaped = true,
                Some(delimiter) if character == delimiter => quote = None,
                Some(_) => {}
                None if matches!(character, '"' | '`' | '\'') => quote = Some(character),
                None if action[index..].starts_with("}}") => {
                    end = index;
                    break;
                }
                None => {}
            }
        }
        actions.push(&action[..end]);
        rest = &action[(end + 2).min(action.len())..];
    }
    actions
}

/// The bare words of an action: identifiers outside strings and comments, not preceded by `.`
/// or `$`.
fn action_words(action: &str) -> Vec<&str> {
    let mut words = Vec::new();
    let bytes = action.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let character = bytes[index];
        // Bytes, not `str` slices: `index` steps through a non-ASCII character a byte at a
        // time, and a slice there would not be on a character boundary.
        if bytes[index..].starts_with(b"/*") {
            index = bytes[index..]
                .windows(2)
                .position(|pair| pair == b"*/")
                .map_or(bytes.len(), |end| index + end + 2);
        } else if matches!(character, b'"' | b'`' | b'\'') {
            let mut end = index + 1;
            while end < bytes.len() && bytes[end] != character {
                end += if character == b'"' && bytes[end] == b'\\' {
                    2
                } else {
                    1
                };
            }
            index = end + 1;
        } else if character.is_ascii_alphabetic() || character == b'_' || !character.is_ascii() {
            // A name may hold letters outside ASCII, as Go's do; it ends at ASCII punctuation,
            // so the slice below stays on character boundaries.
            let start = index;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric()
                    || bytes[index] == b'_'
                    || !bytes[index].is_ascii())
            {
                index += 1;
            }
            if start == 0 || !matches!(bytes[start - 1], b'.' | b'$') {
                words.push(&action[start..index]);
            }
        } else {
            index += 1;
        }
    }
    words
}

/// A name a template file can have: `[a-z0-9][a-z0-9._-]*` ending in `.yaml`, `.yml` or `.tpl`.
/// Only lowercase, since a disk that ignores case would let `Deployment.yaml` replace Alien's
/// `deployment.yaml`.
fn is_template_file_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_lowercase() || first.is_ascii_digit())
        && characters.all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '.' | '_' | '-')
        })
        && [".yaml", ".yml", ".tpl"]
            .iter()
            .any(|extension| name.ends_with(extension))
}

/// The names a template's `define` and `block` actions give the templates they declare.
fn defined_templates(contents: &str) -> Vec<String> {
    let mut defined = Vec::new();
    let mut rest = contents;
    while let Some(start) = rest.find("{{") {
        rest = &rest[start + 2..];
        let action = rest.trim_start_matches('-').trim_start();
        let Some(declaration) = action
            .strip_prefix("define")
            .or_else(|| action.strip_prefix("block"))
        else {
            continue;
        };
        let declaration = declaration.trim_start();
        let Some(quote) = declaration
            .chars()
            .next()
            .filter(|c| matches!(c, '"' | '`'))
        else {
            continue;
        };
        if let Some(end) = declaration[1..].find(quote) {
            defined.push(declaration[1..1 + end].to_string());
        }
    }
    defined
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_cloudformation::CfResource;

    fn template(resources: &[(&str, &str)]) -> CfTemplate {
        let mut template: CfTemplate =
            serde_json::from_value(serde_json::json!({ "AWSTemplateFormatVersion": "2010-09-09" }))
                .unwrap();
        template.resources = resources
            .iter()
            .map(|(id, resource_type)| {
                (
                    id.to_string(),
                    CfResource::new(id.to_string(), resource_type.to_string()),
                )
            })
            .collect();
        template
    }

    fn files(files: &[(&str, &str)]) -> IndexMap<String, String> {
        files
            .iter()
            .map(|(path, contents)| (path.to_string(), contents.to_string()))
            .collect()
    }

    const DB_TF: &str = r#"resource "aws_rds_cluster" "db" {
  engine = "aurora-postgresql"
}

data "aws_region" "current" {}
"#;

    /// The error the pass gives for `reason`, as it names the extension and the modifier.
    fn refusal(format: Format, reason: String) -> String {
        Modifier {
            hooks: &ExtensionHooks::unloaded("acme"),
            hook: "modifier/0",
            format,
        }
        .refuse(reason)
        .message
    }

    /// What the pass checks a Terraform modifier's output against, for a module Alien wrote,
    /// with its refusal as the pass words it.
    fn refused(before: &IndexMap<String, String>, changed: IndexMap<String, String>) -> String {
        refusal(Format::Terraform, modified(before, changed).unwrap_err())
    }

    fn refused_cloudformation(alien: &CfTemplate, modified: &CfTemplate) -> String {
        refusal(
            Format::CloudFormation,
            check_cloudformation(alien, alien, modified).unwrap_err(),
        )
    }

    /// What the pass checks a Terraform modifier's output against, for a module Alien wrote.
    fn modified(
        before: &IndexMap<String, String>,
        changed: IndexMap<String, String>,
    ) -> std::result::Result<IndexMap<String, String>, String> {
        modified_terraform("acme", &AlienModule::read(before).unwrap(), before, changed)
    }

    #[test]
    fn a_cloudformation_resource_may_change_and_new_ones_may_be_added() {
        let before = template(&[("Db", "AWS::RDS::DBCluster")]);
        let mut after = before.clone();
        after.resources["Db"]
            .properties
            .insert("BackupRetentionPeriod".into(), 30.into());
        after
            .resources
            .extend(template(&[("Alarm", "AWS::CloudWatch::Alarm")]).resources);
        after.outputs = serde_json::from_value(
            serde_json::json!({ "AlarmArn": { "Value": { "Ref": "Alarm" } } }),
        )
        .unwrap();
        assert_eq!(check_cloudformation(&before, &before, &after), Ok(()));
    }

    #[test]
    fn a_removed_or_retyped_cloudformation_resource_is_refused() {
        let before = template(&[("Db", "AWS::RDS::DBCluster"), ("Key", "AWS::KMS::Key")]);
        let removed = template(&[("Db", "AWS::RDS::DBCluster")]);
        assert_eq!(
            check_cloudformation(&before, &before, &removed),
            Err("removed Key (AWS::KMS::Key) from the template".to_string())
        );
        let retyped = template(&[("Db", "AWS::RDS::DBInstance"), ("Key", "AWS::KMS::Key")]);
        assert_eq!(
            check_cloudformation(&before, &before, &retyped),
            Err("changed the type of Db from AWS::RDS::DBCluster to AWS::RDS::DBInstance".into())
        );
    }

    #[test]
    fn a_condition_or_policy_on_alien_s_resource_is_refused() {
        let alien = template(&[("Db", "AWS::RDS::DBCluster")]);
        let hide: [(&str, fn(&mut CfResource)); 3] = [
            ("Condition", |db| db.condition = Some("Never".into())),
            ("DeletionPolicy", |db| {
                db.deletion_policy = Some("Delete".into())
            }),
            ("UpdateReplacePolicy", |db| {
                db.update_replace_policy = Some("Delete".into())
            }),
        ];
        for (attribute, change) in hide {
            let mut modified = alien.clone();
            change(&mut modified.resources["Db"]);
            assert_eq!(
                refused_cloudformation(&alien, &modified),
                format!(
                    "the CloudFormation modifier `modifier/0` of extension 'acme' changed the {attribute} of Db, a resource Alien emits"
                )
            );
        }
        // The extension's own resources may have them.
        let mut modified = alien.clone();
        let mut alarm = CfResource::new("Alarm".into(), "AWS::CloudWatch::Alarm".into());
        alarm.condition = Some("InProduction".into());
        modified.resources.insert("Alarm".into(), alarm);
        modified.conditions.insert(
            "InProduction".into(),
            serde_json::from_value(serde_json::json!({ "Fn::Equals": ["a", "a"] })).unwrap(),
        );
        assert_eq!(check_cloudformation(&alien, &alien, &modified), Ok(()));
    }

    #[test]
    fn a_transform_or_nested_stack_is_refused() {
        let alien = template(&[("Db", "AWS::RDS::DBCluster")]);

        let mut modified = alien.clone();
        modified.transform = vec!["AWS::Serverless-2016-10-31".into()];
        assert_eq!(
            refused_cloudformation(&alien, &modified),
            "the CloudFormation modifier `modifier/0` of extension 'acme' changed the template's Transform"
        );

        let mut modified = alien.clone();
        modified.resources["Db"].properties.insert(
            "Tags".into(),
            serde_json::from_value(serde_json::json!({
                "Fn::Transform": { "Name": "AWS::Include", "Parameters": { "Location": "s3://vendor/x.yaml" } }
            }))
            .unwrap(),
        );
        let refusal = refused_cloudformation(&alien, &modified);
        assert!(
            refusal.starts_with(
                "the CloudFormation modifier `modifier/0` of extension 'acme' added an Fn::Transform: "
            ),
            "{refusal}"
        );

        let mut modified = alien.clone();
        let mut nested = CfResource::new("Vendor".into(), "AWS::CloudFormation::Stack".into());
        nested
            .properties
            .insert("TemplateURL".into(), "https://vendor.example/x.yaml".into());
        modified.resources.insert("Vendor".into(), nested);
        assert_eq!(
            refused_cloudformation(&alien, &modified),
            "the CloudFormation modifier `modifier/0` of extension 'acme' added or changed Vendor, a nested stack (AWS::CloudFormation::Stack)"
        );
    }

    /// Rules check the installer's parameters and metadata labels them; the description is what
    /// the installer reads first. A modifier may add rules and metadata, not change Alien's.
    #[test]
    fn alien_s_rules_metadata_and_description_are_kept() {
        let mut alien = template(&[("Db", "AWS::RDS::DBCluster")]);
        alien.description = Some("Alien setup".into());
        alien.metadata.insert(
            "AWS::CloudFormation::Interface".into(),
            serde_json::json!({ "ParameterLabels": { "ManagementExternalId": { "default": "ExternalId" } } }),
        );
        alien.rules = serde_json::from_value(serde_json::json!({
            "ExternalIdSet": { "Assertions": [{
                "Assert": { "Fn::Not": [{ "Fn::Equals": [{ "Ref": "ManagementExternalId" }, ""] }] },
                "AssertDescription": "Enter the ExternalId"
            }] }
        }))
        .unwrap();
        let mut relabeled = alien.clone();
        relabeled.metadata["AWS::CloudFormation::Interface"] = serde_json::json!({ "ParameterLabels": { "ManagementExternalId": { "default": "Optional" } } });
        let mut unruled = alien.clone();
        unruled.rules.clear();
        let mut redescribed = alien.clone();
        redescribed.description = Some("Harmless".into());
        for (modified, expected) in [
            (
                relabeled,
                "changed or removed Metadata.AWS::CloudFormation::Interface",
            ),
            (unruled, "changed or removed Rules.ExternalIdSet"),
            (redescribed, "changed the template's Description"),
        ] {
            let refused = refused_cloudformation(&alien, &modified);
            assert!(refused.contains(expected), "{refused}");
        }

        let mut added = alien.clone();
        added
            .metadata
            .insert("Acme".into(), serde_json::json!({ "zone": true }));
        added.rules.insert(
            "AcmeRule".into(),
            serde_json::from_value(serde_json::json!({ "Assertions": [] })).unwrap(),
        );
        check_cloudformation(&alien, &alien, &added).unwrap();
    }

    /// A module's README ends with what each extension changed, and a module no extension
    /// changed is left as it is.
    #[test]
    fn a_module_s_readme_records_what_extensions_changed() {
        let mut module = ModuleFiles {
            files: IndexMap::from([("README.md".to_string(), "# Setup\n".to_string())]),
        };
        let unchanged = ExtensionChanges {
            extension: "quiet".into(),
            added: vec![],
            changed: vec![],
        };
        record_terraform_changes(&mut module, &[unchanged.clone()]);
        assert_eq!(module.files["README.md"], "# Setup\n");

        let acme = ExtensionChanges {
            extension: "acme".into(),
            added: vec!["acme.tf".into()],
            changed: vec!["db.tf".into()],
        };
        record_terraform_changes(&mut module, &[unchanged, acme]);
        assert_eq!(
            module.files["README.md"],
            "# Setup\n\n## Changed by extensions\n\n- acme: added acme.tf; changed db.tf\n"
        );
    }

    /// A template's text outside ASCII is read a byte at a time, not cut mid-character.
    #[test]
    fn a_non_ascii_template_action_is_read_without_panicking() {
        assert_eq!(
            action_words("/* résumé */ include \"é\" .Values.ünicode"),
            ["include"]
        );
        assert_eq!(action_words("é/*ü*/ printf"), ["é", "printf"]);
    }

    /// A modifier's refusal meets the same template again, so it is never retried.
    #[test]
    fn a_refusal_is_not_retried() {
        let error = Modifier {
            hooks: &ExtensionHooks::unloaded("acme"),
            hook: "modifier/0",
            format: Format::CloudFormation,
        }
        .refuse("removed Db".to_string());
        assert!(!error.retryable, "{error}");
        assert!(!unreadable(Format::Terraform, "x".to_string()).retryable);
    }

    /// A modifier's reference to a resource or parameter the template doesn't define fails the
    /// stack at setup, so it is refused; one Alien's template already has is not the modifier's.
    #[test]
    fn a_reference_to_nothing_is_refused() {
        let mut alien = template(&[("Db", "AWS::RDS::DBCluster")]);
        alien.parameters =
            serde_json::from_value(serde_json::json!({ "Size": { "Type": "String" } })).unwrap();
        alien.mappings =
            serde_json::from_value(serde_json::json!({ "Sizes": { "Small": { "Value": "1" } } }))
                .unwrap();
        alien.conditions =
            serde_json::from_value(serde_json::json!({ "Always": { "Fn::Equals": ["a", "a"] } }))
                .unwrap();
        let with = |property: Value| {
            let mut modified = alien.clone();
            let mut alarm = CfResource::new("Alarm".into(), "AWS::CloudWatch::Alarm".into());
            alarm.properties.insert(
                "Dimension".into(),
                serde_json::from_value(property).unwrap(),
            );
            modified.resources.insert("Alarm".into(), alarm);
            modified
        };
        for (property, missing) in [
            (serde_json::json!({ "Ref": "Cluster" }), "Cluster"),
            (
                serde_json::json!({ "Fn::GetAtt": ["Cache", "Arn"] }),
                "Cache",
            ),
            (
                serde_json::json!({ "Fn::Sub": "arn:${Bucket.Arn}/x" }),
                "Bucket",
            ),
            (serde_json::json!({ "Ref": "AWS::Bogus" }), "AWS::Bogus"),
            (
                serde_json::json!({ "Fn::Sub": "${AWS::Regoin}" }),
                "AWS::Regoin",
            ),
            (
                serde_json::json!({ "Fn::GetAtt": ["Size", "Arn"] }),
                "Size as a resource",
            ),
            (
                serde_json::json!({ "Fn::FindInMap": ["Regions", { "Ref": "AWS::Region" }, "Ami"] }),
                "mapping Regions",
            ),
            (
                serde_json::json!({ "Fn::FindInMap": ["Sizes", "Large", "Value"] }),
                "mapping Sizes.Large",
            ),
            (
                serde_json::json!({ "Fn::FindInMap": ["Sizes", "Small", "Count"] }),
                "mapping Sizes.Small.Count",
            ),
            (
                serde_json::json!({ "Fn::If": ["IsProd", 1, 2] }),
                "condition IsProd",
            ),
        ] {
            let refused = refused_cloudformation(&alien, &with(property));
            assert!(
                refused.contains(&format!("refers to {missing},")),
                "{refused}"
            );
        }
        for fine in [
            serde_json::json!({ "Ref": "Db" }),
            serde_json::json!({ "Ref": "Size" }),
            serde_json::json!({ "Ref": "AWS::Region" }),
            serde_json::json!({ "Ref": "AWS::NoValue" }),
            serde_json::json!({ "Fn::GetAtt": "Db.Endpoint.Address" }),
            serde_json::json!({ "Fn::Sub": ["${Name}-${!Literal}-${Db}", { "Name": "x" }] }),
            serde_json::json!({ "Fn::FindInMap": ["Sizes", "Small", "Value"] }),
            serde_json::json!({ "Fn::FindInMap": ["Sizes", { "Ref": "Size" }, "Value"] }),
            serde_json::json!({ "Fn::FindInMap": ["Sizes", "Small", { "Ref": "Size" }] }),
            serde_json::json!({ "Fn::If": ["Always", 1, 2] }),
        ] {
            check_cloudformation(&alien, &alien, &with(fine)).unwrap();
        }

        // A resource's DependsOn and Condition, and a condition another condition names.
        let mut depends = with(serde_json::json!("x"));
        depends.resources["Alarm"].depends_on = vec!["Queue".into()];
        assert!(refused_cloudformation(&alien, &depends).contains("refers to Queue,"));
        let mut conditional = with(serde_json::json!("x"));
        conditional.resources["Alarm"].condition = Some("IsProd".into());
        assert!(
            refused_cloudformation(&alien, &conditional).contains("refers to condition IsProd,")
        );
        let mut nested = with(serde_json::json!("x"));
        nested.conditions.insert(
            "Both".into(),
            serde_json::from_value(serde_json::json!({
                "Fn::And": [{ "Condition": "Always" }, { "Condition": "IsProd" }]
            }))
            .unwrap(),
        );
        assert!(refused_cloudformation(&alien, &nested).contains("refers to condition IsProd,"));
        let mut fine = with(serde_json::json!("x"));
        fine.resources["Alarm"].depends_on = vec!["Db".into()];
        fine.resources["Alarm"].condition = Some("Always".into());
        check_cloudformation(&alien, &alien, &fine).unwrap();
    }

    /// Each name a modifier adds lands in the package's list of changes, a YAML comment and a
    /// README line, so one that could leave its line is refused.
    #[test]
    fn a_name_a_modifier_adds_is_a_plain_logical_name() {
        let alien = template(&[("Db", "AWS::RDS::DBCluster")]);
        let injected = "x\nTransform: AWS::Serverless-2016-10-31";
        let mut metadata = alien.clone();
        metadata
            .metadata
            .insert(injected.into(), serde_json::json!("y"));
        let refused = refused_cloudformation(&alien, &metadata);
        assert!(refused.contains("added Metadata entry"), "{refused}");
        assert!(!refused.contains('\n'), "{refused}");

        let mut resource = alien.clone();
        resource.resources.insert(
            "My-Alarm".into(),
            CfResource::new("My-Alarm".into(), "AWS::CloudWatch::Alarm".into()),
        );
        assert!(refused_cloudformation(&alien, &resource).contains("added Resources entry"));

        let mut output = alien.clone();
        output.outputs =
            serde_json::from_value(serde_json::json!({ "a b": { "Value": "x" } })).unwrap();
        assert!(refused_cloudformation(&alien, &output).contains("added Outputs entry"));

        // A name Alien's own template has is not the modifier's to answer for.
        let mut alien_metadata = alien.clone();
        alien_metadata.metadata.insert(
            "AWS::CloudFormation::Interface".into(),
            serde_json::json!({}),
        );
        check_cloudformation(&alien_metadata, &alien_metadata, &alien_metadata.clone()).unwrap();

        let lines = changes_for_review(&[ExtensionChanges {
            extension: "acme".into(),
            added: vec![format!("Metadata.{injected}")],
            changed: vec![],
        }]);
        assert_eq!(lines.len(), 1);
        assert!(!lines[0].contains('\n'), "{}", lines[0]);
        assert!(lines[0].contains("x\\nTransform"), "{}", lines[0]);
    }

    #[test]
    fn every_changed_cloudformation_part_is_named() {
        let alien = template(&[("Db", "AWS::RDS::DBCluster")]);
        let mut modified = alien.clone();
        modified.outputs =
            serde_json::from_value(serde_json::json!({ "DbArn": { "Value": "x" } })).unwrap();
        modified.description = Some("changed".into());
        let before = cloudformation_parts(&alien).unwrap();
        let after = cloudformation_parts(&modified).unwrap();
        let added: Vec<_> = after
            .keys()
            .filter(|key| !before.contains_key(*key))
            .collect();
        assert_eq!(added, ["Description", "Outputs.DbArn"]);
    }

    #[test]
    fn a_terraform_modifier_may_change_the_module_s_files_and_add_its_own() {
        let before = files(&[
            ("db.tf", DB_TF),
            ("other.tf", "resource \"aws_route53_zone\" \"other\" {}\n"),
            ("README.md", "# Module"),
        ]);
        let changed = files(&[
            (
                "db.tf",
                &DB_TF.replace("engine", "backup_retention_period = 30\n  engine"),
            ),
            ("acme.tf", "resource \"aws_route53_zone\" \"acme\" {}\n"),
        ]);
        let mut expected = before.clone();
        expected.extend(changed.clone());
        assert_eq!(modified(&before, changed), Ok(expected));
    }

    #[test]
    fn a_terraform_reference_to_nothing_is_refused() {
        let before = files(&[
            ("db.tf", DB_TF),
            (
                "providers.tf",
                "provider \"aws\" {\n  alias = \"west\"\n  region = \"us-west-2\"\n}\n",
            ),
        ]);
        let with = |attribute: &str| {
            files(&[(
                "acme.tf",
                &format!(
                    "resource \"aws_cloudwatch_metric_alarm\" \"acme\" {{\n  {attribute}\n}}\n"
                ),
            )])
        };
        for (attribute, missing) in [
            (
                "dimensions = { cluster = aws_rds_cluster.other.id }",
                "aws_rds_cluster.other",
            ),
            ("alarm_name = \"${var.prefix}-db\"", "var.prefix"),
            ("namespace = local.namespace", "local.namespace"),
            ("count = length(module.backups.ids)", "module.backups"),
            (
                "tags = { region = data.aws_region.other.name }",
                "data.aws_region.other",
            ),
            ("provider = aws.east", "provider aws.east"),
        ] {
            assert!(
                refused(&before, with(attribute)).contains(&format!("refers to {missing},")),
                "{attribute}"
            );
        }
        for fine in [
            "dimensions = { cluster = aws_rds_cluster.db.id }",
            "alarm_name = \"${data.aws_region.current.name}-${path.module}\"",
            "tags = { for key, value in { a = 1 } : key => value }",
            "alarm_actions = [for s in [aws_rds_cluster.db] : s.arn]",
            "provider = aws.west",
            "provider = aws",
            "lifecycle {\n    ignore_changes = [tags]\n  }",
            "description = \"%{for item in [1] }${item.x}%{endfor}\"",
        ] {
            modified(&before, with(fine)).unwrap();
        }
        let dynamic = files(&[(
            "acme.tf",
            "resource \"aws_security_group\" \"acme\" {\n  dynamic \"ingress\" {\n    for_each = [1]\n    content {\n      from_port = ingress.value.port\n    }\n  }\n}\n",
        )]);
        modified(&before, dynamic).unwrap();
        // A module's `providers` values are this module's configurations, which include the
        // aliases a `terraform` block declares; its keys are the child module's.
        let with_aliases = files(&[
            ("db.tf", DB_TF),
            (
                "versions.tf",
                "terraform {\n  required_providers {\n    aws = {\n      source = \"hashicorp/aws\"\n      configuration_aliases = [aws.peer]\n    }\n  }\n}\n",
            ),
        ]);
        let module = |providers: &str| {
            files(&[(
                "acme.tf",
                &format!(
                    "module \"backups\" {{\n  source = \"./backups\"\n  providers = {{ {providers} }}\n}}\n"
                ),
            )])
        };
        modified(&with_aliases, module("aws = aws.peer, aws.dr = aws")).unwrap();
        assert!(refused(&with_aliases, module("aws = aws.east"))
            .contains("refers to provider aws.east,"),);
    }

    /// `provider` and `providers` name provider configurations only at the top of a resource,
    /// data source, ephemeral resource or module, a check block's data sources included.
    /// Anywhere else they are ordinary arguments, whose references must resolve.
    #[test]
    fn provider_is_a_meta_argument_only_on_the_blocks_that_take_one() {
        let before = files(&[
            ("db.tf", DB_TF),
            (
                "providers.tf",
                "provider \"aws\" {\n  alias = \"west\"\n  region = \"us-west-2\"\n}\n",
            ),
        ]);
        let own = |contents: &str| files(&[("acme.tf", contents)]);
        modified(
            &before,
            own("variable \"cloud\" {}\n\nlocals {\n  provider = var.cloud\n}\n"),
        )
        .unwrap();
        for (contents, missing) in [
            ("locals {\n  provider = var.cloud\n}\n", "var.cloud"),
            (
                "resource \"aws_instance\" \"acme\" {\n  settings {\n    provider = local.missing\n  }\n}\n",
                "local.missing",
            ),
            (
                "check \"acme\" {\n  data \"aws_region\" \"peer\" {\n    provider = aws.east\n  }\n}\n",
                "provider aws.east",
            ),
            (
                "module \"acme\" {\n  source    = \"./acme\"\n  providers = { aws = aws.east }\n}\n",
                "provider aws.east",
            ),
        ] {
            let message = refused(&before, own(contents));
            assert!(
                message.contains(&format!("refers to {missing},")),
                "{contents}: {message}"
            );
        }
    }

    /// `configuration_aliases` declares provider configurations; it refers to nothing.
    #[test]
    fn required_providers_refer_to_nothing() {
        let module = files(&[(
            "versions.tf",
            "terraform {\n  required_providers {\n    aws = {\n      source                = \"hashicorp/aws\"\n      configuration_aliases = [aws.peer]\n    }\n  }\n}\n",
        )]);
        assert_eq!(dangling_terraform_references(&module), Ok(BTreeSet::new()));
    }

    /// The labels a modifier adds land in a README line, so one that isn't a Terraform identifier
    /// is refused, and the line escapes what could break it or reorder its text.
    #[test]
    fn a_label_a_terraform_modifier_adds_is_an_identifier() {
        let before = files(&[("db.tf", DB_TF)]);
        let mut in_alien_s_file = DB_TF.to_string();
        in_alien_s_file.push_str("\nresource \"aws_sns_topic\" \"a*b\" {}\n");
        for changed in [
            files(&[(
                "acme.tf",
                "resource \"aws_route53_zone\" \"a](https://example.com)\" {}\n",
            )]),
            files(&[("acme.tf", "resource \"aws route53_zone\" \"acme\" {}\n")]),
            files(&[("acme.tf", "variable \"1st\" {}\n")]),
            files(&[("acme.tf", "output \"a\u{202e}b\" {\n  value = 1\n}\n")]),
            files(&[("db.tf", in_alien_s_file.as_str())]),
        ] {
            let message = refused(&before, changed.clone());
            assert!(
                message.contains("which isn't a Terraform identifier"),
                "{changed:?}: {message}"
            );
            assert!(!message.contains('\u{202e}'), "{message}");
        }
        modified(
            &before,
            files(&[(
                "acme.tf",
                "resource \"aws_route53_zone\" \"acme-zone_1\" {}\n\nvariable \"_x\" {}\n",
            )]),
        )
        .unwrap();

        let lines = changes_for_review(&[ExtensionChanges {
            extension: "acme".into(),
            added: vec![
                "a\u{202e}b".into(),
                "c\u{2028}d".into(),
                "e\u{200b}f".into(),
            ],
            changed: vec![],
        }]);
        for raw in ['\u{202e}', '\u{2028}', '\u{200b}'] {
            assert!(!lines[0].contains(raw), "{:?}", lines[0]);
        }
        assert!(lines[0].contains("a\\u{202e}b"), "{}", lines[0]);
    }

    #[test]
    fn a_terraform_modifier_may_not_write_another_file() {
        let before = files(&[
            ("db.tf", DB_TF),
            ("README.md", "# Module"),
            ("retain-data.py", "print()"),
        ]);
        for path in [
            "other.tf",
            "zz_acme_override.tf",
            "acme.tf.json",
            "nested/acme.tf",
            "README.md",
            "retain-data.py",
        ] {
            assert_eq!(
                refused(&before, files(&[(path, "")])),
                format!(
                    "the Terraform modifier `modifier/0` of extension 'acme' wrote {path}; it may change the module's .tf files and add acme.tf"
                )
            );
        }
    }

    #[test]
    fn a_lost_or_retyped_terraform_address_is_refused() {
        let before = files(&[("db.tf", DB_TF)]);
        for (lost, after) in [
            ("aws_rds_cluster.db", "data \"aws_region\" \"current\" {}\n"),
            (
                "data.aws_region.current",
                "resource \"aws_rds_cluster\" \"db\" {}\n",
            ),
            (
                "aws_rds_cluster.db",
                &DB_TF.replace("aws_rds_cluster", "aws_rds_cluster_instance"),
            ),
        ] {
            assert_eq!(
                modified(&before, files(&[("db.tf", after)])),
                Err(format!("removed {lost} from the module"))
            );
        }
    }

    #[test]
    fn an_address_declared_twice_is_refused_whichever_file_has_it_first() {
        let before = files(&[
            ("db.tf", DB_TF),
            ("first.tf", "resource \"aws_route53_zone\" \"zone\" {}\n"),
        ]);
        for (resource_type, name, first) in [
            ("aws_rds_cluster", "db", "db.tf"),
            ("aws_route53_zone", "zone", "first.tf"),
        ] {
            let redeclared = format!("resource \"{resource_type}\" \"{name}\" {{}}\n");
            assert_eq!(
                modified(&before, files(&[("acme.tf", &redeclared)])),
                Err(format!(
                    "declares {resource_type}.{name} in acme.tf, which {first} declares"
                ))
            );
        }
    }

    #[test]
    fn a_meta_argument_on_alien_s_block_is_refused() {
        let before = files(&[("db.tf", DB_TF)]);
        for (meta, edit) in [
            ("count", "count  = 0\n  engine"),
            ("for_each", "for_each = {}\n  engine"),
            ("provider", "provider = aws.vendor\n  engine"),
            (
                "lifecycle",
                "lifecycle {\n    ignore_changes = all\n  }\n  engine",
            ),
        ] {
            assert_eq!(
                refused(&before, files(&[("db.tf", &DB_TF.replacen("engine", edit, 1))])),
                format!(
                    "the Terraform modifier `modifier/0` of extension 'acme' changed the {meta} of aws_rds_cluster.db, a block Alien emits"
                )
            );
        }
        let with_count = DB_TF.replace(
            "data \"aws_region\" \"current\" {}",
            "data \"aws_region\" \"current\" {\n  count = 1\n}",
        );
        assert_eq!(
            refused(&before, files(&[("db.tf", &with_count)])),
            "the Terraform modifier `modifier/0` of extension 'acme' changed or removed data.aws_region.current in db.tf, which Alien's resources read"
        );
        // The extension's own blocks may use them.
        let own = "resource \"aws_route53_zone\" \"acme\" {\n  count = 0\n}\n";
        assert!(modified(&before, files(&[("acme.tf", own)])).is_ok());
    }

    #[test]
    fn moving_alien_s_block_to_another_file_is_refused() {
        let before = files(&[("db.tf", DB_TF)]);
        let changed = files(&[
            ("db.tf", "data \"aws_region\" \"current\" {}\n"),
            (
                "acme.tf",
                "resource \"aws_rds_cluster\" \"db\" {\n  engine = \"aurora-postgresql\"\n}\n",
            ),
        ]);
        assert_eq!(
            refused(&before, changed),
            "the Terraform modifier `modifier/0` of extension 'acme' moved aws_rds_cluster.db, a block Alien emits, from db.tf to acme.tf"
        );
    }

    #[test]
    fn every_changed_terraform_file_and_block_is_named() {
        let before = files(&[
            ("db.tf", DB_TF),
            ("outputs.tf", "output \"db\" {\n  value = 1\n}\n"),
        ]);
        let mut after = before.clone();
        after["outputs.tf"] = "output \"db\" {\n  value = 2\n}\n".to_string();
        let (before, after) = (
            terraform_parts(&before).unwrap(),
            terraform_parts(&after).unwrap(),
        );
        let changed: Vec<_> = before
            .iter()
            .filter(|(key, part)| after.get(*key) != Some(*part))
            .map(|(key, _)| key.as_str())
            .collect();
        assert_eq!(changed, ["outputs.tf", "output.db in outputs.tf"]);
    }

    /// Variables and locals decide how many of Alien's resources exist (the network's `count`
    /// reads `var.network_mode`), and providers and `terraform` where they go.
    #[test]
    fn alien_s_variables_locals_and_providers_are_frozen() {
        let before = files(&[
            ("db.tf", DB_TF),
            (
                "variables.tf",
                "variable \"network_mode\" {\n  type    = string\n  default = \"create-new\"\n}\n",
            ),
            ("locals.tf", "locals {\n  resource_prefix = var.name\n}\n"),
            ("providers.tf", "provider \"aws\" {\n  region = var.aws_region\n}\n"),
            (
                "versions.tf",
                "terraform {\n  required_providers {\n    aws = {\n      source = \"hashicorp/aws\"\n    }\n  }\n}\n",
            ),
        ]);
        for (path, from, to, refusal) in [
            (
                "variables.tf",
                "\"create-new\"",
                "\"use-default\"",
                "changed or removed variable.network_mode in variables.tf, which Alien's resources read",
            ),
            (
                "locals.tf",
                "var.name",
                "\"vendor\"",
                "changed or removed locals in locals.tf, which Alien's resources read",
            ),
            (
                "providers.tf",
                "var.aws_region",
                "\"eu-north-1\"",
                "changed or removed provider.aws in providers.tf, which Alien's resources read",
            ),
            (
                "versions.tf",
                "hashicorp/aws",
                "vendor/aws",
                "changed or removed terraform in versions.tf, which Alien's resources read",
            ),
        ] {
            assert_eq!(
                refused(&before, files(&[(path, &before[path].replacen(from, to, 1))])),
                format!("the Terraform modifier `modifier/0` of extension 'acme' {refusal}"),
                "{path}: {from} -> {to}"
            );
        }
        assert_eq!(
            refused(&before, files(&[("variables.tf", "")])),
            "the Terraform modifier `modifier/0` of extension 'acme' changed or removed variable.network_mode in variables.tf, which Alien's resources read"
        );
        let added = format!("{}\nvariable \"vendor\" {{}}\n", before["variables.tf"]);
        assert_eq!(
            refused(&before, files(&[("variables.tf", &added)])),
            "the Terraform modifier `modifier/0` of extension 'acme' added variable.vendor to variables.tf, one of Alien's files; an extension declares its own in acme.tf"
        );
        // The extension's own file may declare its own.
        let own = "variable \"acme_tier\" {}\n\nlocals {\n  acme = 1\n}\n\ndata \"aws_region\" \"acme\" {}\n\nmodule \"acme_dns\" {\n  source = \"./acme-dns\"\n}\n";
        assert!(modified(&before, files(&[("acme.tf", own)])).is_ok());
    }

    #[test]
    fn an_extension_s_file_may_not_configure_terraform_or_reach_alien_s_resources() {
        let before = files(&[("db.tf", DB_TF)]);
        for (block, refusal) in [
            (
                "terraform {\n  required_version = \">= 1.0\"\n}\n",
                "declares a terraform block in acme.tf",
            ),
            (
                "provider \"aws\" {\n  region = \"eu-north-1\"\n}\n",
                "declares a provider block in acme.tf",
            ),
            (
                "moved {\n  from = aws_rds_cluster.db\n  to   = aws_rds_cluster.vendor\n}\n",
                "declares a moved block in acme.tf",
            ),
            (
                "removed {\n  from = aws_rds_cluster.db\n}\n",
                "declares a removed block in acme.tf",
            ),
            (
                "import {\n  to = aws_rds_cluster.db\n  id = \"vendor\"\n}\n",
                "declares an import block in acme.tf",
            ),
            (
                "module \"vendor\" {\n  source = \"git::https://vendor.example/x.git\"\n}\n",
                "declares module.vendor in acme.tf with a source that isn't a local path",
            ),
        ] {
            let message = refused(&before, files(&[("acme.tf", block)]));
            assert!(
                message.starts_with(&format!(
                    "the Terraform modifier `modifier/0` of extension 'acme' {refusal}"
                )),
                "{message}"
            );
        }
    }

    /// Alien registers the setup from its outputs.
    #[test]
    fn alien_s_outputs_are_frozen_and_an_extension_may_add_its_own() {
        let before = files(&[
            ("db.tf", DB_TF),
            (
                "outputs.tf",
                "output \"db\" {\n  value = aws_rds_cluster.db.endpoint\n}\n",
            ),
        ]);
        assert_eq!(
            refused(
                &before,
                files(&[(
                    "outputs.tf",
                    "output \"db\" {\n  value = \"vendor.example\"\n}\n"
                )])
            ),
            "the Terraform modifier `modifier/0` of extension 'acme' changed or removed output.db in outputs.tf, which Alien registers"
        );
        let own = "output \"acme_zone\" {\n  value = 1\n}\n";
        assert!(modified(&before, files(&[("acme.tf", own)])).is_ok());

        let alien: CfTemplate = serde_json::from_value({
            let mut value =
                serde_json::to_value(template(&[("Db", "AWS::RDS::DBCluster")])).unwrap();
            value["Outputs"] = serde_json::json!({ "DbEndpoint": { "Value": { "Ref": "Db" } } });
            value
        })
        .unwrap();
        let mut value = serde_json::to_value(&alien).unwrap();
        value["Outputs"]["DbEndpoint"]["Value"] = serde_json::json!("vendor.example");
        assert_eq!(
            refused_cloudformation(&alien, &serde_json::from_value(value).unwrap()),
            "the CloudFormation modifier `modifier/0` of extension 'acme' changed or removed Outputs.DbEndpoint, which Alien registers"
        );
        let mut value = serde_json::to_value(&alien).unwrap();
        value["Outputs"]["AcmeZone"] = serde_json::json!({ "Value": "z" });
        assert_eq!(
            check_cloudformation(&alien, &alien, &serde_json::from_value(value).unwrap()),
            Ok(())
        );
    }

    #[test]
    fn alien_s_conditions_parameters_and_mappings_are_frozen() {
        let mut value = serde_json::to_value(template(&[("Vpc", "AWS::EC2::VPC")])).unwrap();
        value["Resources"]["Vpc"]["Condition"] = serde_json::json!("NetworkModeCreate");
        value["Conditions"] = serde_json::json!({
            "NetworkModeCreate": { "Fn::Equals": [{ "Ref": "NetworkMode" }, "create-new"] }
        });
        value["Parameters"] =
            serde_json::json!({ "NetworkMode": { "Type": "String", "Default": "create-new" } });
        value["Mappings"] = serde_json::json!({ "Regions": { "us-east-1": { "Az": "a" } } });
        let alien: CfTemplate = serde_json::from_value(value.clone()).unwrap();
        for (section, entry, refusal) in [
            (
                "Conditions",
                serde_json::json!({ "NetworkModeCreate": { "Fn::Equals": ["a", "b"] } }),
                "changed or removed Conditions.NetworkModeCreate, which Alien's template reads",
            ),
            (
                "Parameters",
                serde_json::json!({ "NetworkMode": { "Type": "String", "Default": "use-default" } }),
                "changed or removed Parameters.NetworkMode, which Alien's template reads",
            ),
            (
                "Mappings",
                serde_json::json!({ "Regions": { "us-east-1": { "Az": "b" } } }),
                "changed or removed Mappings.Regions, which Alien's template reads",
            ),
        ] {
            let mut changed = value.clone();
            changed[section] = entry;
            let modified: CfTemplate = serde_json::from_value(changed).unwrap();
            assert_eq!(
                refused_cloudformation(&alien, &modified),
                format!("the CloudFormation modifier `modifier/0` of extension 'acme' {refusal}")
            );
        }
        // The extension may add entries of its own.
        let mut added = value.clone();
        added["Conditions"]["AcmeOn"] = serde_json::json!({ "Fn::Equals": ["a", "a"] });
        added["Parameters"]["AcmeTier"] = serde_json::json!({ "Type": "String" });
        let modified: CfTemplate = serde_json::from_value(added).unwrap();
        assert_eq!(check_cloudformation(&alien, &alien, &modified), Ok(()));
    }

    #[test]
    fn a_stack_set_is_refused_like_a_nested_stack() {
        let alien = template(&[("Db", "AWS::RDS::DBCluster")]);
        let mut modified = alien.clone();
        let mut set = CfResource::new("Vendor".into(), "AWS::CloudFormation::StackSet".into());
        set.properties
            .insert("TemplateURL".into(), "https://vendor.example/x.yaml".into());
        modified.resources.insert("Vendor".into(), set);
        assert_eq!(
            refused_cloudformation(&alien, &modified),
            "the CloudFormation modifier `modifier/0` of extension 'acme' added or changed Vendor, a nested stack (AWS::CloudFormation::StackSet)"
        );
    }

    #[test]
    fn a_helm_template_may_not_change_a_value_in_place_or_run_text_as_a_template() {
        let chart = files(&[("templates/deployment.yaml", "kind: Deployment")]);
        for (contents, function) in [
            ("{{- $_ := set .Release \"IsUpgrade\" false -}}", "set"),
            (
                "{{- $r := .Release -}}{{- $_ := set $r \"Namespace\" \"vendor-ns\" -}}",
                "set",
            ),
            ("{{ $_ := unset .Values \"x\" }}", "unset"),
            ("{{ $_ := merge .Values (dict \"a\" 1) }}", "merge"),
            ("{{ mergeOverwrite .Values (dict) }}", "mergeOverwrite"),
            ("{{ mustMerge .Values (dict) }}", "mustMerge"),
            (
                "{{ mustMergeOverwrite .Values (dict) }}",
                "mustMergeOverwrite",
            ),
            ("{{- tpl \"{{ define \\\"x\\\" }}y{{ end }}\" . -}}", "tpl"),
            ("{{ (set .Release \"Name\" \"x\") | toYaml }}", "set"),
        ] {
            let refusal = helm_templates(
                "acme",
                &chart,
                BTreeMap::from([("acme.yaml".to_string(), contents.to_string())]),
            )
            .unwrap_err();
            assert!(
                refusal.starts_with(&format!("calls {function} in acme.yaml;")),
                "{contents}: {refusal}"
            );
        }
        // A word outside an action, in a string or a comment, or as a field or a variable is
        // not a call.
        let harmless = "set: true\nmerge: \"yes\"\nvalue: {{ .Values.set }}\nname: {{ \"set\" }}\n{{/* tpl set */}}\n{{ $set := 1 }}{{ $set }}\n";
        assert!(helm_templates(
            "acme",
            &chart,
            BTreeMap::from([("acme.yaml".to_string(), harmless.to_string())])
        )
        .is_ok());
    }

    #[test]
    fn an_extension_named_like_alien_s_values_or_templates_is_refused() {
        let chart = files(&[(
            "values.yaml",
            "heartbeat:\n  collection: {}\nmanagement:\n  token: \"\"\n",
        )]);
        assert_eq!(
            helm_extension_name("heartbeat", &chart),
            Err("its name is `heartbeat`, a key of the chart's values, which its subchart's values would replace".into())
        );
        assert_eq!(
            helm_extension_name("Management", &chart),
            Err("its name is `management`, a key of the chart's values, which its subchart's values would replace".into())
        );
        assert_eq!(
            helm_extension_name("deployment-tools", &chart),
            Err(
                "its name starts with \"deployment\", the prefix of the chart's named templates"
                    .into()
            )
        );
        assert_eq!(helm_extension_name("acme", &chart), Ok(()));
    }

    #[test]
    fn helm_templates_land_in_the_extension_s_subchart() {
        let chart = files(&[
            ("Chart.yaml", "apiVersion: v2\nname: app\nversion: 0.1.0\n"),
            (
                "values.schema.json",
                "{\n  \"type\": \"object\",\n  \"additionalProperties\": false,\n  \"properties\": {\n    \"image\": { \"type\": \"string\" }\n  }\n}\n",
            ),
            ("templates/deployment.yaml", "kind: Deployment"),
        ]);
        let added = helm_templates(
            "acme",
            &chart,
            BTreeMap::from([(
                "acme-policy.yaml".to_string(),
                "kind: NetworkPolicy".to_string(),
            )]),
        )
        .unwrap();
        let paths: Vec<_> = added.iter().map(|(path, _)| path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "charts/acme/Chart.yaml",
                "charts/acme/values.yaml",
                "Chart.yaml",
                "values.schema.json",
                "charts/acme/templates/acme-policy.yaml"
            ]
        );
        assert!(added[0].1.contains("name: acme\n"), "{}", added[0].1);
        // Helm lints a subchart its parent doesn't list, and the parent's schema sees the
        // subchart's values under its name.
        assert_eq!(
            added[2].1,
            "apiVersion: v2\nname: app\nversion: 0.1.0\ndependencies:\n- name: acme\n  version: 0.0.0\n"
        );
        assert_eq!(
            added[3].1,
            "{\n  \"type\": \"object\",\n  \"additionalProperties\": false,\n  \"properties\": {\n    \"acme\": { \"type\": \"object\" },\n    \"image\": { \"type\": \"string\" }\n  }\n}\n"
        );

        // A second modifier of the extension adds to the subchart it has.
        let mut chart = chart;
        chart.extend(added);
        let more = helm_templates(
            "acme",
            &chart,
            BTreeMap::from([("acme-quota.yml".to_string(), String::new())]),
        )
        .unwrap();
        assert_eq!(more.len(), 1);
        assert_eq!(
            helm_templates(
                "acme",
                &chart,
                BTreeMap::from([("ACME-Policy.yaml".to_string(), String::new())])
            )
            .unwrap_err(),
            "returned a template named \"ACME-Policy.yaml\"; a name is lowercase letters, digits, '.', '_' and '-', and ends in .yaml, .yml or .tpl"
        );
        assert_eq!(
            helm_templates(
                "acme",
                &chart,
                BTreeMap::from([("acme-policy.yaml".to_string(), String::new())])
            )
            .unwrap_err(),
            "returned charts/acme/templates/acme-policy.yaml, which the chart already has"
        );
    }

    #[test]
    fn a_helm_template_name_is_a_plain_lowercase_template_file() {
        let chart = files(&[("templates/deployment.yaml", "kind: Deployment")]);
        for name in [
            "DEPLOYMENT.yaml",
            "Deployment.yaml",
            "deployment.yaml ",
            "deployment.yaml.",
            "../Chart.yaml",
            "nested/policy.yaml",
            "a\\b.yaml",
            "",
            ".yaml",
            "-x.yaml",
            "policy.json",
            "NOTES.txt",
        ] {
            let refusal = helm_templates(
                "acme",
                &chart,
                BTreeMap::from([(name.to_string(), String::new())]),
            )
            .unwrap_err();
            assert!(
                refusal.starts_with("returned a template named"),
                "{name:?}: {refusal}"
            );
        }
    }

    #[test]
    fn a_named_template_outside_the_extension_s_prefix_is_refused() {
        let chart = files(&[(
            "templates/_helpers.tpl",
            "{{- define \"deployment.labels\" -}}{{- end }}",
        )]);
        for contents in [
            "{{- define \"deployment.labels\" -}}app: x{{- end }}",
            "{{define `deployment.labels`}}x{{end}}",
            "{{ block \"deployment.fullname\" . }}x{{ end }}",
            "{{- define \"acmex.labels\" }}{{ end }}",
        ] {
            let refusal = helm_templates(
                "acme",
                &chart,
                BTreeMap::from([("acme-helpers.tpl".to_string(), contents.to_string())]),
            )
            .unwrap_err();
            assert!(
                refusal.starts_with("defines the named template"),
                "{contents}: {refusal}"
            );
        }
        assert!(helm_templates(
            "acme",
            &chart,
            BTreeMap::from([(
                "acme-helpers.tpl".to_string(),
                "{{- define \"acme.labels\" -}}app: acme{{- end }}{{ include \"deployment.labels\" . }}".to_string()
            )])
        )
        .is_ok());
    }

    #[test]
    fn changes_list_each_extension_by_name() {
        let changes = ExtensionChanges {
            extension: "acme".into(),
            added: vec!["charts/acme/templates/acme-policy.yaml".into()],
            changed: vec!["Db".into(), "Key".into()],
        };
        assert_eq!(
            changes.to_string(),
            "acme: added charts/acme/templates/acme-policy.yaml; changed Db, Key"
        );
        let none = ExtensionChanges {
            extension: "tags".into(),
            added: vec![],
            changed: vec![],
        };
        assert_eq!(none.to_string(), "tags: no changes");
    }
}
