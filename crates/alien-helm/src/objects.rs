//! The objects a chart renders, as Helm prints them, and the checks that a stack's extensions
//! leave Alien's objects as they are.
//!
//! An extension's Helm templates live in its own subchart, `charts/<extension>/`. Before
//! publishing, a host renders the chart twice, with and without the subcharts' templates, and
//! compares the objects by kind and name. Before installing, a host reads the release Helm is
//! about to apply, after it resolved the release's state, the cluster's capabilities and its
//! lookups, and refuses one where two objects share a kind and name.

use std::collections::BTreeMap;
use yaml_rust2::parser::{Event, Parser};
use yaml_rust2::scanner::TScalarStyle;

/// One object `helm template` rendered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedObject {
    /// The template it came from, as Helm's `# Source:` comment names it
    pub source: String,
    pub kind: String,
    pub name: String,
}

/// The objects in `helm template` output, document by document, each from the template its
/// `# Source:` comment names.
fn rendered_objects_reason(rendered: &str) -> Result<Vec<RenderedObject>, String> {
    let mut objects = Vec::new();
    for document in rendered.split("\n---") {
        let Some(source) = document
            .lines()
            .find_map(|line| line.strip_prefix("# Source: "))
        else {
            continue;
        };
        let value = read_document(document)
            .map_err(|error| format!("helm template rendered {source} as {error}"))?;
        objects.extend(document_objects(value, source));
    }
    Ok(objects)
}

/// The annotation Helm puts on every object it hands a post-renderer, naming its template.
const POST_RENDER_SOURCE: &str = "postrenderer.helm.sh/postrender-filename";

/// The objects in what Helm hands a post-renderer: every document of the release, hooks
/// included, each annotated with its template.
fn post_rendered_objects_reason(manifest: &str) -> Result<Vec<RenderedObject>, String> {
    let mut objects = Vec::new();
    for document in manifest.split("\n---") {
        let value = read_document(document)
            .map_err(|error| format!("Helm rendered a document as {error}"))?;
        let source = value["metadata"]["annotations"][POST_RENDER_SOURCE]
            .as_str()
            .unwrap_or("a template Helm didn't name")
            .to_string();
        objects.extend(document_objects(value, &source));
    }
    Ok(objects)
}

/// A document as Kubernetes reads it, once [`refuse_tags`] passed it.
fn read_document(document: &str) -> Result<serde_yaml::Value, String> {
    refuse_tags(document)?;
    serde_yaml::from_str(document).map_err(|error| format!("YAML that does not read: {error}"))
}

/// The handle YAML's own tags (`!!binary`) resolve to.
const YAML_TAGS: &str = "tag:yaml.org,2002:";

/// Characters YAML 1.1 readers (libyaml, Go's yaml.v3, sigs.k8s.io/yaml) take as line breaks
/// and YAML 1.2 readers, the scanner below among them, take as text.
const YAML_1_1_LINE_BREAKS: [char; 3] = ['\u{85}', '\u{2028}', '\u{2029}'];

fn merge_key_refused() -> String {
    "YAML with a merge key (<<), which a chart may not use".to_string()
}

/// Refuses a document with a YAML tag (`!!binary`, `!x`, `!<...>`) on any node, or a merge key
/// (`<<: *anchor`). Kubernetes' decoder resolves some tags, such as `!!binary`, that serde_yaml
/// drops or keeps as a value of their own, and applies merge keys, which serde_yaml doesn't; a
/// tagged or merged node can name an object a check here would not see. Helm hands a
/// post-renderer every plain `<<` as `!!merge <<`, and `helm template` prints it as written;
/// either is refused as a key and read as text elsewhere, as Kubernetes reads it.
///
/// The scanner reads YAML 1.2, where U+0085, U+2028 and U+2029 are text; Kubernetes reads
/// YAML 1.1, where they end a line, so a tag after one would reach Kubernetes unseen here. A
/// document with any of them is refused.
fn refuse_tags(document: &str) -> Result<(), String> {
    if let Some(character) = document.chars().find(|c| YAML_1_1_LINE_BREAKS.contains(c)) {
        return Err(format!(
            "YAML with the line break U+{:04X}, which a chart may not use",
            u32::from(character)
        ));
    }
    // For each open collection, whether it is a mapping whose next node is a key.
    let mut open: Vec<Option<bool>> = Vec::new();
    let mut parser = Parser::new_from_str(document);
    loop {
        let (event, _) = parser
            .next_token()
            .map_err(|error| format!("YAML that does not read: {error}"))?;
        let is_key = matches!(open.last(), Some(Some(true)));
        let (tag, ends_node) = match event {
            Event::StreamEnd => return Ok(()),
            Event::Scalar(value, style, _, tag) => {
                let merge_tagged = tag
                    .as_ref()
                    .is_some_and(|tag| tag.handle == YAML_TAGS && tag.suffix == "merge");
                let plain = matches!(style, TScalarStyle::Plain) && tag.is_none();
                if value == "<<" && (plain || merge_tagged) {
                    if is_key {
                        return Err(merge_key_refused());
                    }
                    (None, true)
                } else {
                    (tag, true)
                }
            }
            Event::Alias(_) => (None, true),
            Event::SequenceStart(_, tag) => {
                open.push(None);
                (tag, false)
            }
            Event::MappingStart(_, tag) => {
                open.push(Some(true));
                (tag, false)
            }
            Event::SequenceEnd | Event::MappingEnd => {
                open.pop();
                (None, true)
            }
            _ => (None, false),
        };
        if ends_node {
            if let Some(Some(next_is_key)) = open.last_mut() {
                *next_is_key = !*next_is_key;
            }
        }
        let Some(tag) = tag else { continue };
        return Err(match tag.handle.as_str() {
            YAML_TAGS => format!(
                "YAML with the tag !!{}, which a chart may not use",
                tag.suffix
            ),
            handle => format!(
                "YAML with the tag {handle}{}, which a chart may not use",
                tag.suffix
            ),
        });
    }
}

/// The objects of one document from `source`. The items of a `List` count as objects of their
/// own, since Kubernetes applies each.
fn document_objects(value: serde_yaml::Value, source: &str) -> Vec<RenderedObject> {
    let mut objects = Vec::new();
    let mut pending = vec![value];
    while let Some(value) = pending.pop() {
        if value["kind"]
            .as_str()
            .is_some_and(|kind| kind.ends_with("List"))
        {
            if let Some(items) = value["items"].as_sequence() {
                pending.extend(items.iter().cloned());
                continue;
            }
        }
        if let (Some(kind), Some(name)) =
            (value["kind"].as_str(), value["metadata"]["name"].as_str())
        {
            objects.push(RenderedObject {
                source: source.to_string(),
                kind: kind.to_string(),
                name: name.to_string(),
            });
        }
    }
    objects
}

/// Refuses a release's `objects` (from [`post_rendered_objects`]) when two share a kind and
/// name, whatever namespace each names: one would replace the other as Kubernetes applies
/// them. A cluster-scoped kind has a name only once in a cluster, and which kinds are
/// cluster-scoped depends on the cluster's custom resources, so the namespace is never part of
/// the comparison. The error names the extensions whose subcharts render them.
fn refuse_repeated_objects_reason(
    objects: &[RenderedObject],
    extensions: &[&str],
) -> Result<(), String> {
    let mut seen = BTreeMap::<(&str, &str), &RenderedObject>::new();
    for object in objects {
        let key = (object.kind.as_str(), object.name.as_str());
        let Some(first) = seen.insert(key, object) else {
            continue;
        };
        let mut owners: Vec<&str> = [first, object]
            .into_iter()
            .filter_map(|object| {
                extensions.iter().copied().find(|extension| {
                    object
                        .source
                        .contains(&format!("/{}", extension_subchart(extension)))
                })
            })
            .collect();
        owners.dedup();
        let what = format!(
            "{} {} ({} and {})",
            object.kind, object.name, first.source, object.source
        );
        let why = "objects of one kind and name collide whatever namespace each names";
        return Err(match owners.as_slice() {
            [] => format!("the release renders {what} twice; {why}"),
            [extension] => format!(
                "extension '{extension}' renders {what}, a kind and name the release already has; {why}"
            ),
            [first, second, ..] => {
                format!("extensions '{first}' and '{second}' both render {what}; {why}")
            }
        });
    }
    Ok(())
}

/// The path of an extension's subchart in its parent chart.
pub fn extension_subchart(extension: &str) -> String {
    format!("charts/{extension}/")
}

/// Refuses `with` (the chart rendered with the subcharts of `extensions`) unless it has each of
/// `alien`'s objects (the chart rendered without the subcharts' templates) as many times by
/// kind and name, outside the subcharts, and no subchart object has the kind and name of one
/// of `alien`'s.
fn compare_extension_objects_reason(
    alien: Vec<RenderedObject>,
    with: Vec<RenderedObject>,
    extensions: &[&str],
) -> Result<(), String> {
    let count = |objects: Vec<RenderedObject>| {
        let mut counts = BTreeMap::<(String, String), usize>::new();
        for object in objects {
            *counts.entry((object.kind, object.name)).or_default() += 1;
        }
        counts
    };
    let alien = count(alien);
    let mut kept = Vec::new();
    for object in with {
        let owner = extensions.iter().find(|extension| {
            object
                .source
                .contains(&format!("/{}", extension_subchart(extension)))
        });
        match owner {
            Some(extension) if alien.contains_key(&(object.kind.clone(), object.name.clone())) => {
                return Err(format!(
                    "extension '{extension}' adds {} {} ({}), which the chart Alien generates already has",
                    object.kind, object.name, object.source
                ));
            }
            Some(_) => {}
            None => kept.push(object),
        }
    }
    let kept = count(kept);
    match alien
        .iter()
        .find(|(object, expected)| kept.get(*object) != Some(*expected))
    {
        Some(((kind, name), expected)) => Err(format!(
            "with the stack's extensions ({}), the chart renders {kind} {name} {} times, and {expected} without them",
            extensions.join(", "),
            kept.get(&(kind.clone(), name.clone())).copied().unwrap_or(0)
        )),
        None => Ok(()),
    }
}

/// A chart a check refuses, the same way on every try.
fn refused(reason: String) -> alien_error::AlienError<alien_core::ErrorData> {
    alien_error::AlienError::new(alien_core::ErrorData::RenderedChartRefused { reason })
}

/// Each object of a chart `helm template` rendered, by kind and name, with the template it came
/// from. Fails on a document it can't read.
pub fn rendered_objects(rendered: &str) -> alien_core::Result<Vec<RenderedObject>> {
    rendered_objects_reason(rendered).map_err(refused)
}

/// Each object of a release's manifest as Helm applies it, from a post-renderer's input.
pub fn post_rendered_objects(manifest: &str) -> alien_core::Result<Vec<RenderedObject>> {
    post_rendered_objects_reason(manifest).map_err(refused)
}

/// Refuses a release's `objects` when two share a kind and name; see
/// [`refuse_repeated_objects_reason`].
pub fn refuse_repeated_objects(
    objects: &[RenderedObject],
    extensions: &[&str],
) -> alien_core::Result<()> {
    refuse_repeated_objects_reason(objects, extensions).map_err(refused)
}

/// Refuses a chart whose extensions drop or take one of Alien's objects; see
/// [`compare_extension_objects_reason`].
pub fn compare_extension_objects(
    alien: Vec<RenderedObject>,
    with: Vec<RenderedObject>,
    extensions: &[&str],
) -> alien_core::Result<()> {
    compare_extension_objects_reason(alien, with, extensions).map_err(refused)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(source: &str, kind: &str, name: &str) -> RenderedObject {
        RenderedObject {
            source: source.to_string(),
            kind: kind.to_string(),
            name: name.to_string(),
        }
    }

    fn alien() -> Vec<RenderedObject> {
        vec![
            object("app/templates/deployment.yaml", "Deployment", "app"),
            object("app/templates/rbac.yaml", "ClusterRole", "app-nodes"),
        ]
    }

    #[test]
    fn alien_s_objects_must_render_the_same_with_the_extensions() {
        let mut with = alien();
        with.push(object(
            "app/charts/acme/templates/policy.yaml",
            "NetworkPolicy",
            "acme",
        ));
        assert_eq!(
            compare_extension_objects_reason(alien(), with, &["acme"]),
            Ok(())
        );

        let missing = vec![object("app/templates/deployment.yaml", "Deployment", "app")];
        assert_eq!(
            compare_extension_objects_reason(alien(), missing, &["acme"]),
            Err("with the stack's extensions (acme), the chart renders ClusterRole app-nodes 0 times, and 1 without them".to_string())
        );
        let mut doubled = alien();
        doubled.push(object("app/templates/deployment.yaml", "Deployment", "app"));
        assert_eq!(
            compare_extension_objects_reason(alien(), doubled, &["acme"]),
            Err("with the stack's extensions (acme), the chart renders Deployment app 2 times, and 1 without them".to_string())
        );
        let mut taken = alien();
        taken.push(object(
            "app/charts/acme/templates/x.yaml",
            "Deployment",
            "app",
        ));
        assert_eq!(
            compare_extension_objects_reason(alien(), taken, &["acme"]),
            Err("extension 'acme' adds Deployment app (app/charts/acme/templates/x.yaml), which the chart Alien generates already has".to_string())
        );
    }

    #[test]
    fn a_list_s_items_are_objects() {
        let rendered = "---\n# Source: app/charts/acme/templates/x.yaml\napiVersion: v1\nkind: List\nitems:\n  - apiVersion: apps/v1\n    kind: Deployment\n    metadata:\n      name: app\n---\n# Source: app/templates/deployment.yaml\napiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: app\n";
        let objects = rendered_objects_reason(rendered).unwrap();
        assert_eq!(
            objects,
            [
                object("app/charts/acme/templates/x.yaml", "Deployment", "app"),
                object("app/templates/deployment.yaml", "Deployment", "app"),
            ]
        );
        let alien = vec![object("app/templates/deployment.yaml", "Deployment", "app")];
        assert!(compare_extension_objects_reason(alien, objects, &["acme"])
            .unwrap_err()
            .starts_with("extension 'acme' adds Deployment app"));
    }

    /// What Helm hands a post-renderer: every document, hooks included, annotated with its
    /// template, and no `# Source:` comments.
    const POST_RENDERED: &str = "apiVersion: v1\nkind: Secret\nmetadata:\n  name: rel\n  annotations:\n    postrenderer.helm.sh/postrender-filename: 'app/templates/secret.yaml'\n---\napiVersion: batch/v1\nkind: Job\nmetadata:\n  name: migrate\n  namespace: other\n  annotations:\n    \"helm.sh/hook\": pre-install\n    postrenderer.helm.sh/postrender-filename: 'app/templates/job.yaml'\n";

    #[test]
    fn a_post_rendered_release_s_objects_carry_their_templates() {
        let objects = post_rendered_objects_reason(POST_RENDERED).unwrap();
        assert_eq!(
            objects,
            [
                object("app/templates/secret.yaml", "Secret", "rel"),
                object("app/templates/job.yaml", "Job", "migrate"),
            ]
        );
        assert_eq!(refuse_repeated_objects_reason(&objects, &["acme"]), Ok(()));
    }

    /// Alien's `Secret rel`, followed by an extension's document with `metadata`.
    fn with_extension_document(document: &str) -> String {
        format!(
            "apiVersion: v1\nkind: Secret\nmetadata:\n  name: rel\n  annotations:\n    postrenderer.helm.sh/postrender-filename: 'app/templates/secret.yaml'\n---\n{document}"
        )
    }

    /// YAML that names an object in ways a Kubernetes decoder reads as plain fields.
    #[test]
    fn every_yaml_form_of_a_name_is_read() {
        for (form, metadata) in [
            ("alias", "  labels:\n    a: &n rel\n  name: *n\n"),
            ("quoted", "  name: \"rel\"\n"),
        ] {
            let manifest = with_extension_document(&format!(
                "apiVersion: v1\nkind: Secret\nmetadata:\n{metadata}  annotations:\n    postrenderer.helm.sh/postrender-filename: 'app/charts/acme/templates/zz.yaml'\n"
            ));
            let objects = post_rendered_objects_reason(&manifest).unwrap();
            assert!(
                refuse_repeated_objects_reason(&objects, &["acme"]).is_err(),
                "{form}: {objects:?}"
            );
        }
    }

    /// Kubernetes' decoder reads each of these as `Secret rel`; serde_yaml drops `!!binary`
    /// and keeps a local tag as a value of its own, so a document with any tag is refused.
    #[test]
    fn a_document_with_a_yaml_tag_is_refused() {
        let annotations = "  annotations:\n    postrenderer.helm.sh/postrender-filename: 'app/charts/acme/templates/zz.yaml'\n";
        for (form, document, tag) in [
            (
                "binary name",
                format!("apiVersion: v1\nkind: Secret\nmetadata:\n  name: !!binary cmVs\n{annotations}"),
                "!!binary",
            ),
            (
                "tagged metadata key",
                format!("apiVersion: v1\nkind: Secret\n!x metadata:\n  name: rel\n{annotations}"),
                "!x",
            ),
            (
                "tagged kind key",
                format!("apiVersion: v1\n!x kind: Secret\nmetadata:\n  name: rel\n{annotations}"),
                "!x",
            ),
            (
                "tagged string",
                format!("apiVersion: v1\nkind: !!str Secret\nmetadata:\n  name: rel\n{annotations}"),
                "!!str",
            ),
            (
                "verbatim tag",
                format!("apiVersion: v1\nkind: Secret\nmetadata:\n  name: !<tag:yaml.org,2002:str> rel\n{annotations}"),
                "tag:yaml.org,2002:str",
            ),
            (
                "merge tag on a name",
                format!("apiVersion: v1\nkind: Secret\nmetadata:\n  name: !!merge rel\n{annotations}"),
                "!!merge",
            ),
            (
                "tagged mapping",
                format!("apiVersion: v1\nkind: Secret\nmetadata: !!map\n  name: rel\n{annotations}"),
                "!!map",
            ),
            (
                "tagged key in a List item",
                "apiVersion: v1\nkind: List\nitems:\n  - apiVersion: v1\n    kind: Secret\n    !x metadata:\n      name: rel\n".to_string(),
                "!x",
            ),
        ] {
            let error = post_rendered_objects_reason(&with_extension_document(&document)).unwrap_err();
            assert!(
                error.starts_with("Helm rendered a document as YAML with the tag")
                    && error.contains(tag),
                "{form}: {error}"
            );
            let rendered = format!("---\n# Source: app/charts/acme/templates/zz.yaml\n{document}");
            let error = rendered_objects_reason(&rendered).unwrap_err();
            assert!(
                error.starts_with("helm template rendered app/charts/acme/templates/zz.yaml as YAML with the tag")
                    && error.contains(tag),
                "{form}: {error}"
            );
        }
        // A `!` inside a quoted or block scalar, or a comment, is text, not a tag.
        let untagged = "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: notes # !x\ndata:\n  a: \"!x\"\n  b: '!!binary'\n  c: |\n    !x not a tag\n  d: x!y\n";
        assert_eq!(
            post_rendered_objects_reason(untagged).unwrap(),
            [object("a template Helm didn't name", "ConfigMap", "notes")]
        );
    }

    /// Kubernetes applies a merge key, which serde_yaml doesn't, so a document with one is
    /// refused: as `helm template` prints it at publish, and as Helm hands it to a
    /// post-renderer at install.
    #[test]
    fn a_document_with_a_merge_key_is_refused_wherever_it_is_read() {
        let merge_key = "YAML with a merge key (<<), which a chart may not use";
        for (form, metadata) in [
            ("as written", "  labels: &m\n    name: rel\n  <<: *m\n"),
            (
                "as Helm hands it",
                "  labels: &m\n    name: rel\n  !!merge <<: *m\n",
            ),
            (
                "a list of merges",
                "  labels: &m\n    name: rel\n  <<: [*m]\n",
            ),
        ] {
            let document = format!("apiVersion: v1\nkind: Secret\nmetadata:\n{metadata}");
            assert_eq!(
                post_rendered_objects_reason(&document),
                Err(format!("Helm rendered a document as {merge_key}")),
                "{form}"
            );
            let rendered =
                format!("---\n# Source: app/charts/acme/templates/merge.yaml\n{document}");
            assert_eq!(
                rendered_objects_reason(&rendered),
                Err(format!(
                    "helm template rendered app/charts/acme/templates/merge.yaml as {merge_key}"
                )),
                "{form}"
            );
        }
        // A merge key after a node that ends with an alias or a collection is still a key.
        for (form, metadata) in [
            (
                "after an alias value",
                "  labels: &m\n    name: rel\n  annotations: *m\n  <<: *m\n",
            ),
            (
                "after a flow sequence value",
                "  labels: &m\n    name: rel\n  finalizers: [a, b]\n  <<: *m\n",
            ),
        ] {
            let document = format!("apiVersion: v1\nkind: Secret\nmetadata:\n{metadata}");
            assert_eq!(
                post_rendered_objects_reason(&document),
                Err(format!("Helm rendered a document as {merge_key}")),
                "{form}"
            );
        }
        // Quoted, `<<` is a key like any other, and as a value or a list item it is text,
        // whether written plain or as Helm hands it to a post-renderer, `!!merge <<`.
        let quoted = "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: notes\ndata:\n  \"<<\": x\n  script: cat <<EOF\n  op: <<\n  flow: {a: <<, b: [<<]}\n  pair: [<<, <<]\n  tagged: !!merge <<\n  items:\n    - !!merge <<\n";
        assert_eq!(
            post_rendered_objects_reason(quoted).unwrap(),
            [object("a template Helm didn't name", "ConfigMap", "notes")]
        );
    }

    /// Kubernetes reads U+0085, U+2028 and U+2029 as line breaks and the tag scan reads them as
    /// text, so a tag or merge key after one would reach Kubernetes unseen: a document with any
    /// of them is refused at publish and at install.
    #[test]
    fn a_document_with_a_yaml_1_1_line_break_is_refused_wherever_it_is_read() {
        let head = "apiVersion: v1\nkind: Secret\n";
        for (form, document, code) in [
            (
                "NEL before a tagged key",
                format!("{head}foo: bar # c\u{85}!x metadata:\u{85}  name: rel\n"),
                "0085",
            ),
            (
                "LS before a tagged key",
                format!("{head}foo: bar # c\u{2028}!x metadata:\u{2028}  name: rel\n"),
                "2028",
            ),
            (
                "PS before a tagged key",
                format!("{head}foo: bar # c\u{2029}!x metadata:\u{2029}  name: rel\n"),
                "2029",
            ),
            (
                "NEL before a binary name",
                format!("{head}metadata:\n  labels: {{}} # c\u{85}  name: !!binary cmVs\n"),
                "0085",
            ),
            (
                "NEL before a merge key",
                format!("{head}metadata:\n  labels: &m {{name: rel}}\n  x: y # c\u{85}  <<: *m\n"),
                "0085",
            ),
        ] {
            let refused = format!("YAML with the line break U+{code}, which a chart may not use");
            assert_eq!(
                post_rendered_objects_reason(&document),
                Err(format!("Helm rendered a document as {refused}")),
                "{form}"
            );
            let rendered = format!("---\n# Source: app/charts/acme/templates/zz.yaml\n{document}");
            assert_eq!(
                rendered_objects_reason(&rendered),
                Err(format!(
                    "helm template rendered app/charts/acme/templates/zz.yaml as {refused}"
                )),
                "{form}"
            );
        }
    }

    #[test]
    fn a_repeated_kind_and_name_is_refused_naming_the_extension() {
        let objects = vec![
            object("app/templates/secret.yaml", "Secret", "rel"),
            object("app/charts/acme/templates/zz.yaml", "Secret", "rel"),
        ];
        assert_eq!(
            refuse_repeated_objects_reason(&objects, &["acme"]),
            Err("extension 'acme' renders Secret rel (app/templates/secret.yaml and app/charts/acme/templates/zz.yaml), a kind and name the release already has; objects of one kind and name collide whatever namespace each names".to_string())
        );
        let other_kind = vec![
            object("app/templates/secret.yaml", "Secret", "rel"),
            object("app/charts/acme/templates/zz.yaml", "ConfigMap", "rel"),
        ];
        assert_eq!(
            refuse_repeated_objects_reason(&other_kind, &["acme"]),
            Ok(())
        );
    }

    /// A namespace never tells two objects apart: Alien renders cluster-scoped kinds of custom
    /// resources, such as Karpenter's `NodePool`, that no list of Kubernetes' own kinds has.
    #[test]
    fn objects_of_one_kind_and_name_collide_whatever_their_namespaces() {
        for (alien, extension) in [
            (
                "apiVersion: karpenter.sh/v1\nkind: NodePool\nmetadata:\n  name: general\n",
                "apiVersion: karpenter.sh/v1\nkind: NodePool\nmetadata:\n  name: general\n  namespace: acme\n",
            ),
            (
                "apiVersion: v1\nkind: Secret\nmetadata:\n  name: rel\n",
                "apiVersion: v1\nkind: Secret\nmetadata:\n  name: rel\n  namespace: \"\"\n",
            ),
            (
                "apiVersion: v1\nkind: Secret\nmetadata:\n  name: rel\n  namespace: ns\n",
                "apiVersion: v1\nkind: Secret\nmetadata:\n  name: rel\n  namespace: acme\n",
            ),
            (
                "apiVersion: rbac.authorization.k8s.io/v1\nkind: ClusterRole\nmetadata:\n  name: app-nodes\n",
                "apiVersion: rbac.authorization.k8s.io/v1\nkind: ClusterRole\nmetadata:\n  name: app-nodes\n  namespace: acme\n",
            ),
        ] {
            let manifest = format!(
                "{alien}  annotations:\n    postrenderer.helm.sh/postrender-filename: 'app/templates/x.yaml'\n---\n{extension}  annotations:\n    postrenderer.helm.sh/postrender-filename: 'app/charts/acme/templates/zz.yaml'\n"
            );
            let objects = post_rendered_objects_reason(&manifest).unwrap();
            let error = refuse_repeated_objects_reason(&objects, &["acme"]).unwrap_err();
            assert!(
                error.starts_with("extension 'acme' renders ")
                    && error.ends_with("collide whatever namespace each names"),
                "{extension}: {error}"
            );
        }
    }

    #[test]
    fn a_collision_between_two_extensions_names_both() {
        let objects = vec![
            object("app/charts/acme/templates/a.yaml", "ConfigMap", "shared"),
            object("app/charts/tags/templates/b.yaml", "ConfigMap", "shared"),
        ];
        assert_eq!(
            refuse_repeated_objects_reason(&objects, &["acme", "tags"]),
            Err("extensions 'acme' and 'tags' both render ConfigMap shared (app/charts/acme/templates/a.yaml and app/charts/tags/templates/b.yaml); objects of one kind and name collide whatever namespace each names".to_string())
        );
    }
}
