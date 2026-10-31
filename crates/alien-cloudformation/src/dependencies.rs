//! The order CloudFormation creates a template's resources in.
//!
//! A resource waits for every resource it names in `DependsOn` and every resource its
//! properties reach through `Ref`, `Fn::GetAtt` or `Fn::Sub`. CloudFormation rejects a
//! template whose resources wait on each other in a cycle, including a resource that waits on
//! itself, so the generator checks the final template before handing it out.

use crate::template::{CfExpression, CfTemplate};
use alien_core::{ErrorData, Result};
use alien_error::AlienError;
use indexmap::{IndexMap, IndexSet};

/// Each resource's logical id, mapped to the resources it waits for.
pub(crate) type DependencyGraph = IndexMap<String, IndexSet<String>>;

pub(crate) fn resource_dependencies(template: &CfTemplate) -> DependencyGraph {
    template
        .resources
        .iter()
        .map(|(logical_id, resource)| {
            let mut dependencies: IndexSet<String> = resource.depends_on.iter().cloned().collect();
            for expression in resource.properties.values() {
                collect_references(expression, &mut dependencies);
            }
            dependencies.retain(|dependency| template.resources.contains_key(dependency));
            (logical_id.clone(), dependencies)
        })
        .collect()
}

/// The resources along the first cycle found, starting and ending with the same logical id.
pub(crate) fn find_cycle(graph: &DependencyGraph) -> Option<Vec<String>> {
    #[derive(Clone, Copy, PartialEq)]
    enum Visit {
        InProgress,
        Done,
    }

    let mut visits: IndexMap<&str, Visit> = IndexMap::new();
    for start in graph.keys() {
        if visits.contains_key(start.as_str()) {
            continue;
        }
        // Each frame is a resource on the current path and the index of its next dependency.
        let mut path: Vec<(&str, usize)> = vec![(start, 0)];
        visits.insert(start, Visit::InProgress);
        while let Some((logical_id, next)) = path.last_mut() {
            let logical_id: &str = logical_id;
            let dependency = graph
                .get(logical_id)
                .and_then(|dependencies| dependencies.get_index(*next));
            *next += 1;
            let Some(dependency) = dependency else {
                visits.insert(logical_id, Visit::Done);
                path.pop();
                continue;
            };
            match visits.get(dependency.as_str()) {
                Some(Visit::Done) => {}
                Some(Visit::InProgress) => {
                    let first = path
                        .iter()
                        .position(|(on_path, _)| *on_path == dependency.as_str())
                        .expect("an in-progress resource is on the current path");
                    let mut cycle: Vec<String> = path[first..]
                        .iter()
                        .map(|(on_path, _)| on_path.to_string())
                        .collect();
                    cycle.push(dependency.clone());
                    return Some(cycle);
                }
                None => {
                    visits.insert(dependency.as_str(), Visit::InProgress);
                    path.push((dependency.as_str(), 0));
                }
            }
        }
    }
    None
}

/// Rejects a template CloudFormation would refuse because its resources wait on each other.
pub(crate) fn validate_acyclic(template: &CfTemplate) -> Result<()> {
    match find_cycle(&resource_dependencies(template)) {
        None => Ok(()),
        Some(cycle) => Err(AlienError::new(ErrorData::TemplateSerializationFailed {
            format: "CloudFormation".to_string(),
            reason: format!(
                "resources depend on each other in a cycle: {}",
                cycle.join(" -> ")
            ),
        })),
    }
}

pub(crate) fn collect_references(expression: &CfExpression, references: &mut IndexSet<String>) {
    match expression {
        CfExpression::Object(values) => {
            if let (1, Some(CfExpression::String(target))) = (values.len(), values.get("Ref")) {
                references.insert(target.clone());
                return;
            }
            if let (1, Some(target)) = (values.len(), values.get("Fn::GetAtt")) {
                match target {
                    CfExpression::List(parts) => {
                        if let Some(CfExpression::String(logical_id)) = parts.first() {
                            references.insert(logical_id.clone());
                        }
                    }
                    CfExpression::String(path) => {
                        if let Some((logical_id, _)) = path.split_once('.') {
                            references.insert(logical_id.to_string());
                        }
                    }
                    _ => {}
                }
                return;
            }
            if let (1, Some(substitution)) = (values.len(), values.get("Fn::Sub")) {
                let (template, variables) = match substitution {
                    CfExpression::String(template) => (template.as_str(), None),
                    CfExpression::List(arguments) => match arguments.as_slice() {
                        [CfExpression::String(template)] => (template.as_str(), None),
                        [CfExpression::String(template), CfExpression::Object(variables)] => {
                            (template.as_str(), Some(variables))
                        }
                        _ => return,
                    },
                    _ => return,
                };
                for name in substitution_names(template) {
                    // A variable map entry overrides the whole placeholder, `Bucket.Arn` included.
                    if !variables.is_some_and(|variables| variables.contains_key(name)) {
                        let logical_id = name.split_once('.').map_or(name, |(id, _)| id);
                        references.insert(logical_id.to_string());
                    }
                }
                for value in variables.into_iter().flat_map(IndexMap::values) {
                    collect_references(value, references);
                }
                return;
            }
            for value in values.values() {
                collect_references(value, references);
            }
        }
        CfExpression::List(values) => {
            for value in values {
                collect_references(value, references);
            }
        }
        CfExpression::Null
        | CfExpression::Bool(_)
        | CfExpression::Integer(_)
        | CfExpression::Number(_)
        | CfExpression::String(_) => {}
    }
}

/// The placeholders an `Fn::Sub` template substitutes: `${Name}` and `${Name.Attribute}`, not
/// the `${!Literal}` escape.
fn substitution_names(mut template: &str) -> Vec<&str> {
    let mut names = Vec::new();
    while let Some(start) = template.find("${") {
        let variable = &template[start + 2..];
        let Some(end) = variable.find('}') else {
            break;
        };
        let name = &variable[..end];
        if !name.starts_with('!') {
            names.push(name);
        }
        template = &variable[end + 1..];
    }
    names
}

#[cfg(test)]
mod tests {
    use super::{find_cycle, resource_dependencies};
    use crate::template::{CfExpression, CfResource, CfTemplate};

    fn template(resources: &[(&str, &[&str], CfExpression)]) -> CfTemplate {
        let mut template = CfTemplate::default();
        for (logical_id, depends_on, property) in resources {
            let mut resource = CfResource::new(logical_id.to_string(), "AWS::S3::Bucket".into());
            resource.depends_on = depends_on.iter().map(|id| id.to_string()).collect();
            resource.properties.insert("Value".into(), property.clone());
            template.resources.insert(logical_id.to_string(), resource);
        }
        template
    }

    #[test]
    fn reads_each_way_a_resource_names_another() {
        let template = template(&[
            ("Role", &[], CfExpression::Null),
            ("Bucket", &[], CfExpression::Null),
            ("Key", &[], CfExpression::Null),
            (
                "Policy",
                &["Key"],
                CfExpression::list([
                    CfExpression::ref_("Role"),
                    CfExpression::get_att("Bucket", "Arn"),
                    CfExpression::sub("${AWS::Region} ${Bucket.Arn} ${!Literal} ${Missing}"),
                    CfExpression::ref_("AWS::StackName"),
                ]),
            ),
        ]);
        let dependencies = resource_dependencies(&template);
        assert_eq!(
            dependencies["Policy"].iter().collect::<Vec<_>>(),
            ["Key", "Role", "Bucket"]
        );
    }

    #[test]
    fn a_sub_variable_overriding_a_dotted_placeholder_is_not_a_reference() {
        let template = template(&[(
            "Bucket",
            &[],
            CfExpression::sub_with(
                "${Bucket.Arn}/*",
                [("Bucket.Arn", CfExpression::sub("arn:aws:s3:::example"))],
            ),
        )]);
        assert!(resource_dependencies(&template)["Bucket"].is_empty());
        assert_eq!(find_cycle(&resource_dependencies(&template)), None);
    }

    /// JSON text is lowered to `Fn::Join` before the cycle check. Its literal pieces are plain
    /// strings, so a resource name written there, even as `${Fake}`, is not a reference; the
    /// intrinsics it carries still are.
    #[test]
    fn lowered_json_text_references_only_its_intrinsics() {
        let mut template = template(&[
            ("Role", &[], CfExpression::Null),
            ("Bucket", &[], CfExpression::Null),
            ("Api", &[], CfExpression::Null),
            ("Fake", &[], CfExpression::Null),
            (
                "Registration",
                &[],
                CfExpression::to_json_string(CfExpression::object([
                    ("role", CfExpression::ref_("Role")),
                    ("bucket", CfExpression::get_att("Bucket", "Arn")),
                    ("url", CfExpression::sub("https://${Api}/${!Fake}")),
                    ("literal", CfExpression::from("${Fake} Ref Fake")),
                ])),
            ),
        ]);
        crate::json_text::lower_json_strings(&mut template).unwrap();
        let value = &template.resources["Registration"].properties["Value"];
        assert!(
            matches!(value, CfExpression::Object(fields) if fields.contains_key("Fn::Join")),
            "lowered to Fn::Join: {value:?}"
        );
        assert_eq!(
            resource_dependencies(&template)["Registration"]
                .iter()
                .collect::<Vec<_>>(),
            ["Role", "Bucket", "Api"]
        );
    }

    #[test]
    fn finds_self_and_longer_cycles_and_accepts_a_diamond() {
        let self_cycle = template(&[("Policy", &["Policy"], CfExpression::Null)]);
        assert_eq!(
            find_cycle(&resource_dependencies(&self_cycle)),
            Some(vec!["Policy".to_string(), "Policy".to_string()])
        );

        let through_a_reference = template(&[
            ("A", &["B"], CfExpression::Null),
            ("B", &[], CfExpression::get_att("C", "Arn")),
            ("C", &["A"], CfExpression::Null),
        ]);
        assert_eq!(
            find_cycle(&resource_dependencies(&through_a_reference)),
            Some(["A", "B", "C", "A"].map(String::from).to_vec())
        );

        let diamond = template(&[
            ("Top", &["Left", "Right"], CfExpression::Null),
            ("Left", &["Bottom"], CfExpression::Null),
            ("Right", &["Bottom"], CfExpression::Null),
            ("Bottom", &[], CfExpression::Null),
        ]);
        assert_eq!(find_cycle(&resource_dependencies(&diamond)), None);
    }
}
