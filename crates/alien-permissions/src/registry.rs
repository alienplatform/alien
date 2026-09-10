//! Built-in permission sets registry
//!
//! This module provides access to the built-in permission sets that are compiled
//! into the alien-permissions crate from JSONC files at build time.
//!
//! ## How it works
//!
//! The registry is automatically generated at build time by scanning all `.jsonc` files
//! in the `permission-sets/` directory. Each JSONC file defines a permission set with
//! platform-specific permissions and binding instructions.
//!
//! ## Generation process
//!
//! 1. **Build script** (`build.rs`) runs during compilation
//! 2. **Scans** `permission-sets/` directory recursively for `.jsonc` files  
//! 3. **Parses** each file using `json5` to extract the permission set ID and content
//! 4. **Generates** Rust code that creates a static registry with all permission sets
//! 5. **Compiles** the generated code into the crate at build time
//!
//! ## Directory structure
//!
//! ```text
//! permission-sets/
//! ├── storage/
//! │   ├── data-read.jsonc
//! │   ├── data-write.jsonc
//! │   ├── management.jsonc
//! │   └── provision.jsonc
//! ├── worker/
//! │   ├── execute.jsonc
//! │   ├── management.jsonc
//! │   ├── provision.jsonc
//! │   └── pull-images.jsonc
//! └── build/
//!     ├── execute.jsonc
//!     ├── management.jsonc
//!     └── provision.jsonc
//! ```
//!
//! ## Usage examples
//!
//! ```rust
//! use alien_permissions::{get_permission_set, list_permission_set_ids, has_permission_set};
//!
//! // Check if a permission set exists
//! if has_permission_set("storage/data-read") {
//!     println!("Permission set exists!");
//! }
//!
//! // Get a permission set by ID
//! if let Some(perm_set) = get_permission_set("storage/data-read") {
//!     println!("Description: {}", perm_set.description);
//!     
//!     // Access AWS permissions
//!     if let Some(aws_perms) = &perm_set.platforms.aws {
//!         for perm in aws_perms {
//!             if let Some(actions) = &perm.grant.actions {
//!                 println!("AWS actions: {:?}", actions);
//!             }
//!         }
//!     }
//! }
//!
//! // List all available permission sets
//! let all_ids = list_permission_set_ids();
//! println!("Available permission sets: {:?}", all_ids);
//! ```
//!
//! ## Adding new permission sets
//!
//! To add a new permission set:
//!
//! 1. Create a new `.jsonc` file in the appropriate subdirectory under `permission-sets/`
//! 2. Define the permission set structure following the schema in `alien-core::permissions::PermissionSet`
//! 3. Rebuild the crate - the build script will automatically include the new permission set
//!
//! Example permission set structure:
//!
//! ```jsonc
//! {
//!   "id": "my-resource/my-action",
//!   "description": "Allows performing my action on my resource",
//!   "platforms": {
//!     "aws": [
//!       {
//!         "grant": {
//!           "actions": ["myservice:MyAction"]
//!         },
//!         "binding": {
//!           "stack": {
//!             "resources": ["arn:aws:myservice:${awsRegion}:${awsAccountId}:myresource/${stackPrefix}-*"]
//!           },
//!           "resource": {
//!             "resources": ["arn:aws:myservice:${awsRegion}:${awsAccountId}:myresource/${resourceName}"]
//!           }
//!         }
//!       }
//!     ]
//!   }
//! }
//! ```
//!
//! ## Technical details
//!
//! - Permission sets are loaded into a static `HashMap` using `once_cell::sync::Lazy`
//! - JSONC parsing is done at build time using the `json5` crate
//! - Generated constants use raw string literals with `###` delimiters to avoid escaping issues
//! - The registry workers return references to static data, so there's no runtime allocation
//! - Changes to permission set files automatically trigger rebuilds via `cargo:rerun-if-changed`

// Include the generated registry code
// This includes the static PERMISSION_SETS_REGISTRY and the public API workers
include!(concat!(env!("OUT_DIR"), "/permission_sets_registry.rs"));

/// AWS actions that hand out a credential reaching inside a MicroVM session.
///
/// A MicroVM auth token is what the sandbox agent protocol travels on, and `ConnectMicrovm`
/// attaches to a running session with no matching API operation to audit.
pub const SENSITIVE_MICROVM_ACTIONS: &[&str] = &[
    "lambda:CreateMicrovmAuthToken",
    "lambda:CreateMicrovmShellAuthToken",
    "lambda:ConnectMicrovm",
];

/// AWS actions that address an existing MicroVM session, or start one.
///
/// Starting counts: one image serves every session of a sandbox and AWS scopes a mint no finer,
/// so whoever holds `sandbox/remote-execute` mints into whatever sessions exist.
pub const MICROVM_SESSION_LIFECYCLE_ACTIONS: &[&str] = &[
    "lambda:RunMicrovm",
    "lambda:SuspendMicrovm",
    "lambda:ResumeMicrovm",
    "lambda:TerminateMicrovm",
    "lambda:GetMicrovm",
];

/// Whether `permission_set` grants anything that addresses a sandbox session, on any cloud it
/// declares. Bindings are skipped — `${stackPrefix}` is uninterpolated this early and ARNs are
/// free-form, so any comparison is unsound. Checked per-cloud rather than by one verb list: AWS
/// names actions, Azure grants reach through a role or `dataActions`.
pub fn permission_set_reaches_a_sandbox_session(
    permission_set: &alien_core::permissions::PermissionSet,
) -> bool {
    let reaches_on_aws = permission_set
        .platforms
        .aws
        .iter()
        .flatten()
        .filter(|entry| entry.effect.is_allow())
        .flat_map(|entry| entry.grant.actions.iter().flatten())
        .any(|action| action_reaches_a_microvm_session(action));

    let reaches_on_azure = permission_set
        .platforms
        .azure
        .iter()
        .flatten()
        .any(azure_entry_reaches_a_sandbox_session);

    let reaches_on_gcp = permission_set
        .platforms
        .gcp
        .iter()
        .flatten()
        .any(gcp_entry_reaches_a_sandbox_session);

    reaches_on_aws || reaches_on_azure || reaches_on_gcp
}

/// Predefined GCP roles that carry `aiplatform.sandboxEnvironments.execute`, read off the live
/// role definitions rather than inferred from the names. It is also why `sandbox/remote-execute`
/// renders a custom role instead of naming one of them.
///
/// Re-derive with: describe every `roles/aiplatform.*`, `roles/ml*`, `roles/notebooks*`,
/// `roles/discoveryengine*` and `roles/geminienterprise*` role plus the basic roles, and keep the
/// ones whose `includedPermissions` contain that verb.
const GCP_SESSION_REACHING_ROLES: &[&str] = &[
    "roles/owner",
    "roles/editor",
    "roles/aiplatform.user",
    "roles/aiplatform.admin",
    "roles/aiplatform.editor",
    "roles/aiplatform.expressAdmin",
    "roles/aiplatform.expressUser",
    "roles/aiplatform.customCodeServiceAgent",
    "roles/aiplatform.serviceAgent",
    "roles/discoveryengine.serviceAgent",
];

/// The one `roles/aiplatform.*` role proven to carry no session verb. Everything else under that
/// prefix is treated as reaching, because a list of names cannot answer for a role that does not
/// exist yet and this answer decides whether the single-tenancy gate refuses a stack.
const GCP_AIPLATFORM_ROLES_WITHOUT_SESSION_REACH: &[&str] = &["roles/aiplatform.viewer"];

/// Three ways to say yes — a custom role (unbounded by name), an `aiplatform.*` role added after
/// this list was written, or one of the roles enumerated above — because guessing wrong here lets
/// a second identity hold `execute` as though single-tenant. Everything else answers no.
fn gcp_role_reaches_a_sandbox_session(role: &str) -> bool {
    if GCP_AIPLATFORM_ROLES_WITHOUT_SESSION_REACH
        .iter()
        .any(|safe| safe.eq_ignore_ascii_case(role))
    {
        return false;
    }
    !role.starts_with("roles/")
        || role.starts_with("roles/aiplatform.")
        || GCP_SESSION_REACHING_ROLES
            .iter()
            .any(|known| known.eq_ignore_ascii_case(role))
}

/// Whether one GCP permission entry reaches a sandbox session, by role or by permission.
fn gcp_entry_reaches_a_sandbox_session(
    entry: &alien_core::permissions::GcpPlatformPermission,
) -> bool {
    entry
        .grant
        .predefined_roles
        .iter()
        .flatten()
        .any(|role| gcp_role_reaches_a_sandbox_session(role))
        // Both lists, unconditionally: an entry setting `permissions` and `residualPermissions`
        // together would otherwise hide the grant in the unscanned one.
        || entry
            .grant
            .permissions
            .iter()
            .flatten()
            .chain(entry.grant.residual_permissions.iter().flatten())
            .any(|permission| permission_reaches_a_sandbox_session(permission))
}

/// Whether one GCP permission, possibly carrying a `*`, addresses a sandbox session.
///
/// **Lifecycle.** Counts as reach for the same reason `RunMicrovm` does on AWS: whoever creates a
/// session can put whatever it likes inside it.
///
/// **`get` and `list` do not.** They report a session's existence and state and confer nothing
/// over it or inside it, so a status-only grant stays with the deployment's own identity rather
/// than being claimed by the remote caller.
///
/// **Wildcard, both directions.** `*` and `aiplatform.*` sit above the sandbox namespace and still
/// reach into it; `aiplatform.sandboxEnvironments.*` sits below. Either way a wildcard covers a
/// verb past `get`/`list`, so it is answered yes — over-approximating withholds a grant, while
/// under-approximating leaves reach on an identity a second tenant holds.
///
/// Compared lowercased throughout, as the Azure helper is.
fn permission_reaches_a_sandbox_session(permission: &str) -> bool {
    const SANDBOX_NAMESPACE: &str = "aiplatform.sandboxenvironments.";
    let permission = permission.to_ascii_lowercase();
    if permission.contains('*') {
        let literal = permission.split('*').next().unwrap_or_default();
        return SANDBOX_NAMESPACE.starts_with(literal) || literal.starts_with(SANDBOX_NAMESPACE);
    }
    permission
        .strip_prefix(SANDBOX_NAMESPACE)
        .is_some_and(|verb| !matches!(verb, "get" | "list"))
}

/// The predefined role carrying Azure's whole sandbox data plane, session contents included.
/// Public because four places must agree on it — this predicate, the invariant test, the role
/// allowlist an author may name from, and the Azure role-id map — or a second data-plane role
/// added to one silently drops out of the others.
pub const AZURE_SANDBOX_DATA_PLANE_ROLE: &str = "Container Apps SandboxGroup Data Owner";

/// Whether one Azure permission entry reaches a sandbox session, by role or by data action.
fn azure_entry_reaches_a_sandbox_session(
    entry: &alien_core::permissions::AzurePlatformPermission,
) -> bool {
    entry
        .grant
        .predefined_roles
        .iter()
        .flatten()
        .any(|role| role.eq_ignore_ascii_case(AZURE_SANDBOX_DATA_PLANE_ROLE))
        || entry
            .grant
            .data_actions
            .iter()
            .flatten()
            .any(|action| data_action_reaches_a_sandbox_session(action))
}

/// Whether one Azure `dataAction`, possibly carrying a `*`, addresses a sandbox session.
///
/// **Lifecycle.** Counts as reach for the same reason `RunMicrovm` does on AWS: whoever starts a
/// session can put whatever it likes inside it.
///
/// **Wildcard, both directions.** `*` and `Microsoft.App/*` sit above the sandbox namespace and
/// still reach into it; `…/sandboxes/*` sits below. Checking only downwards would clear the two
/// that matter most, so every verb under the namespace counts rather than a suffix allowlist.
fn data_action_reaches_a_sandbox_session(action: &str) -> bool {
    const SANDBOX_NAMESPACE: &str = "microsoft.app/sandboxgroups/sandboxes";
    let action = action.to_ascii_lowercase();
    if action.contains('*') {
        let literal = action.split('*').next().unwrap_or_default();
        return SANDBOX_NAMESPACE.starts_with(literal) || literal.starts_with(SANDBOX_NAMESPACE);
    }
    action.starts_with(SANDBOX_NAMESPACE)
}

/// Whether one IAM action, possibly carrying a `*`, can authorize an operation on a session.
///
/// **Wildcard.** Cleared against the known verbs *and* the MicroVM namespace — clearing against
/// today's names alone would miss `lambda:SomeFutureMicrovmVerb*`, letting the broader grant slip
/// through.
///
/// **Exact action.** Matched on the MicroVM namespace, so a verb AWS adds later fails closed.
///
/// **`MicrovmImage`.** Excluded: it addresses the image a session launches from, which
/// `sandbox/provision` and `sandbox/heartbeat` legitimately hold.
///
/// Compared lowercased throughout — AWS matches action names case-insensitively.
fn action_reaches_a_microvm_session(action: &str) -> bool {
    // IAM matches an action name with two wildcards, `*` for many characters and `?` for one.
    // Reading only `*` sends `lambda:CreateMicrov?AuthToken` down the exact branch, where it
    // matches no verb and is answered no while authorizing the real one.
    let action = action.to_ascii_lowercase().replace('?', "*");
    if action.contains('*') {
        let literal = action.split('*').next().unwrap_or_default();
        let covers_a_known_verb = SENSITIVE_MICROVM_ACTIONS
            .iter()
            .chain(MICROVM_SESSION_LIFECYCLE_ACTIONS)
            .any(|known| known.to_ascii_lowercase().starts_with(literal));
        // Every literal run counts, not only the one before the first `*`: `lambda:Foo*Microvm`
        // names the namespace after the wildcard. Each run is cleared the same way, so the
        // `MicrovmImage` exclusion still applies to whichever run carries the name.
        return covers_a_known_verb || action.split('*').any(reaches_the_microvm_namespace);
    }
    action
        .strip_prefix("lambda:")
        .is_some_and(reaches_the_microvm_namespace)
}

/// Whether a `lambda:`-stripped verb addresses a MicroVM rather than its image.
fn reaches_the_microvm_namespace(verb: &str) -> bool {
    let verb = verb.strip_prefix("lambda:").unwrap_or(verb);
    verb.contains("microvm") && !verb.contains("microvmimage")
}

/// Whether `permission_set_id` grants anything at all on `platform`.
///
/// An absent or empty block is a kind the platform does not support: emitters and the generated
/// permission docs both iterate the block, so such a grant installs no role binding and prints a
/// heading with no permissions under it.
pub fn permission_set_covers_platform(
    permission_set_id: &str,
    platform: alien_core::Platform,
) -> bool {
    let Some(permission_set) = get_permission_set(permission_set_id) else {
        return false;
    };
    let platforms = &permission_set.platforms;
    let entries = match platform {
        alien_core::Platform::Aws => platforms.aws.as_ref().map(Vec::len),
        alien_core::Platform::Gcp => platforms.gcp.as_ref().map(Vec::len),
        alien_core::Platform::Azure => platforms.azure.as_ref().map(Vec::len),
        alien_core::Platform::Kubernetes
        | alien_core::Platform::Machines
        | alien_core::Platform::Local
        | alien_core::Platform::Test => None,
    };
    entries.is_some_and(|count| count > 0)
}

/// Whether a scope naming `${resourceName}` is bounded to that one resource.
///
/// Either IAM wildcard inside the token's own name segment spans siblings:
/// `${stackPrefix}-${resourceName}-*` renders `acme-agents-*`, which matches sibling sandbox
/// `agents-2`'s `acme-agents-2`. Only `/` and `:` end that segment — no id or prefix holds either.
#[cfg(test)]
fn names_one_resource(scope: &str) -> bool {
    const TOKEN: &str = "${resourceName}";
    scope.split_once(TOKEN).is_some_and(|(_, rest)| {
        let segment = rest.split(['/', ':']).next().unwrap_or_default();
        !segment.contains('*') && !segment.contains('?')
    })
}

/// Whether the session-reaching part of a set is scoped to the resource it is filed under.
///
/// The single-tenancy gate treats a **named** set as scoped by the profile key it sits under. That
/// only holds where every entry carrying the session-reaching grant names `${resourceName}` and
/// bounds the segment it sits in: one scoped to a whole project or subscription, or one whose
/// wildcard runs past the name, reaches siblings whatever key it is filed under. Entries that reach
/// no session are not consulted — `sandbox/remote-execute` binds AWS's own network connector by a
/// fixed ARN, which names no sandbox and grants nothing inside one.
///
/// Consulted for AWS and Azure, the platforms whose named sets the gate scopes by key. GCP has no
/// counterpart invariant — its session-reaching sets are not all pinned to one resource — so
/// `reaches_this_sandbox` reads a named GCP set as inline.
#[cfg(test)]
fn permission_set_is_resource_scoped_on(
    permission_set: &alien_core::permissions::PermissionSet,
    platform: alien_core::Platform,
) -> bool {
    let platforms = &permission_set.platforms;
    match platform {
        alien_core::Platform::Aws => platforms
            .aws
            .iter()
            .flatten()
            .filter(|entry| {
                entry.effect.is_allow()
                    && entry
                        .grant
                        .actions
                        .iter()
                        .flatten()
                        .any(|action| action_reaches_a_microvm_session(action))
            })
            .all(|entry| {
                entry.binding.resource.as_ref().is_some_and(|spec| {
                    spec.resources
                        .iter()
                        .all(|resource| names_one_resource(resource))
                })
            }),
        alien_core::Platform::Azure => platforms
            .azure
            .iter()
            .flatten()
            .filter(|entry| azure_entry_reaches_a_sandbox_session(entry))
            .all(|entry| {
                entry
                    .binding
                    .resource
                    .as_ref()
                    .is_some_and(|spec| names_one_resource(&spec.scope))
            }),
        alien_core::Platform::Gcp => platforms
            .gcp
            .iter()
            .flatten()
            .filter(|entry| gcp_entry_reaches_a_sandbox_session(entry))
            .all(|entry| {
                entry
                    .binding
                    .resource
                    .as_ref()
                    .is_some_and(|spec| names_one_resource(&spec.scope))
            }),
        // A permission set declares no block for these, so it carries no entry to scope.
        alien_core::Platform::Kubernetes
        | alien_core::Platform::Machines
        | alien_core::Platform::Local
        | alien_core::Platform::Test => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        permissions::PermissionProfile, Resource, ResourceEntry, ResourceLifecycle, Storage,
    };

    fn storage(remote_access: bool) -> ResourceEntry {
        ResourceEntry {
            enabled_when: None,
            config: Resource::new(Storage::new("assets".to_string()).build()),
            dependencies: Vec::new(),
            lifecycle: ResourceLifecycle::Frozen,
            remote_access,
        }
    }

    /// The filter reaches every remote binding kind, not only sandbox, so this pins the case that
    /// carries the rest of the product: a deployment that declares no remote binding at all keeps
    /// exactly the management reach it had.
    #[test]
    fn a_deployment_without_remote_bindings_keeps_every_global_management_ref() {
        let profile =
            PermissionProfile::new().global(["storage/remote-data-write", "worker/provision"]);
        let resources = [storage(false)];

        let kept = management_identity_global_refs(resources.iter(), &profile);

        assert_eq!(
            kept.len(),
            2,
            "nothing is claimed when nothing is bound remotely: {:?}",
            kept.iter().map(|r| r.id()).collect::<Vec<_>>()
        );
    }

    /// A remote storage binding carries its own set on the caller's identity. Leaving it on the
    /// management identity too is the reach setup deliberately withheld.
    #[test]
    fn a_remote_storage_binding_claims_its_set_from_the_management_identity() {
        let profile =
            PermissionProfile::new().global(["storage/remote-data-write", "worker/provision"]);
        let resources = [storage(true)];

        let kept = management_identity_global_refs(resources.iter(), &profile);

        let ids = kept.iter().map(|r| r.id()).collect::<Vec<_>>();
        assert!(
            !ids.contains(&"storage/remote-data-write"),
            "the remotely bound set stays off the management identity: {ids:?}"
        );
        assert!(
            ids.contains(&"worker/provision"),
            "and a set nothing claims is untouched: {ids:?}"
        );
    }

    use super::*;

    #[test]
    fn test_registry_contains_expected_permission_sets() {
        // Test that some known permission sets exist
        assert!(has_permission_set("storage/data-read"));
        assert!(has_permission_set("storage/data-write"));
        assert!(has_permission_set("storage/management"));
        assert!(has_permission_set("storage/provision"));
        assert!(has_permission_set("worker/execute"));
        assert!(has_permission_set("worker/management"));
        assert!(has_permission_set("worker/provision"));
        assert!(has_permission_set("build/execute"));
        assert!(has_permission_set("build/management"));
        assert!(has_permission_set("build/provision"));
        assert!(has_permission_set("kubernetes-cluster/heartbeat"));
        assert!(has_permission_set("email/provision"));
        assert!(has_permission_set("email/heartbeat"));
        assert!(has_permission_set("email/send"));
        assert!(has_permission_set("email/management"));
        assert!(has_permission_set("email/manage-identities"));
    }

    #[test]
    fn test_get_permission_set() {
        let storage_read = get_permission_set("storage/data-read");
        assert!(storage_read.is_some());

        let perm_set = storage_read.unwrap();
        assert_eq!(perm_set.id, "storage/data-read");
        assert_eq!(
            perm_set.description,
            "Allows reading data from storage buckets and containers"
        );

        // Check that it has platforms defined
        assert!(perm_set.platforms.aws.is_some());
        assert!(perm_set.platforms.gcp.is_some());
        assert!(perm_set.platforms.azure.is_some());
    }

    #[test]
    fn test_nonexistent_permission_set() {
        assert!(!has_permission_set("nonexistent/permission"));
        assert!(get_permission_set("nonexistent/permission").is_none());
    }

    #[test]
    fn test_list_permission_set_ids() {
        let ids = list_permission_set_ids();
        assert!(!ids.is_empty());
        assert!(ids.contains(&"storage/data-read"));
        assert!(ids.contains(&"worker/execute"));

        // Should be sorted or at least consistent
        println!("Available permission sets: {:?}", ids);
    }

    /// A resource id may be another id plus a separator — `agents` and `agents-2` are both valid,
    /// and both render an image named `{prefix}-{id}` — so a wildcard left in the segment the
    /// resource name sits in matches the sibling's resource and the gate's key scoping is void.
    #[test]
    fn only_a_bounded_resource_name_segment_names_one_resource() {
        for bounded in [
            "arn:aws:lambda:us-east-1:1:microvm-image:acme-${resourceName}",
            "arn:aws:lambda:us-east-1:1:microvm-image:acme-${resourceName}:*",
            "arn:aws:s3:::${resourceName}/sandbox/*",
            "/subscriptions/s/resourceGroups/g/providers/Microsoft.App/sandboxGroups/${resourceName}",
        ] {
            assert!(
                names_one_resource(bounded),
                "'{bounded}' names one resource and nothing beside it"
            );
        }

        for reaches_a_sibling in [
            "arn:aws:lambda:us-east-1:1:microvm-image:acme-${resourceName}*",
            "arn:aws:lambda:us-east-1:1:microvm-image:acme-${resourceName}-*",
            "arn:aws:lambda:us-east-1:1:microvm-image:acme-${resourceName}-?",
            "arn:aws:lambda:us-east-1:1:microvm-image:acme-${resourceName}_*",
            "arn:aws:lambda:us-east-1:1:microvm-image:acme-${resourceName}-build*",
        ] {
            assert!(
                !names_one_resource(reaches_a_sibling),
                "'{reaches_a_sibling}' also matches the resource a sibling id renders"
            );
        }

        assert!(
            !names_one_resource("/subscriptions/${subscriptionId}"),
            "a scope that never names the resource is bounded to no resource"
        );
    }

    /// The single-tenancy gate scopes a **named** session-reaching set by the profile key it sits
    /// under, which only holds while every such set's resource binding names `${resourceName}`.
    /// A set that reaches a session through a project- or subscription-wide resource binding has
    /// to fall through to the inline treatment instead, so this pins the assumption at the source.
    ///
    /// GCP is out of the loop: its session-reaching sets are not all pinned to one resource, so
    /// `reaches_this_sandbox` reads a named GCP set as inline rather than scoping it by key.
    #[test]
    fn every_session_reaching_set_is_resource_scoped_by_resource_name() {
        for id in list_permission_set_ids() {
            let permission_set = get_permission_set(id).expect("a listed set resolves");
            if !permission_set_reaches_a_sandbox_session(permission_set) {
                continue;
            }
            for platform in [alien_core::Platform::Aws, alien_core::Platform::Azure] {
                assert!(
                    permission_set_is_resource_scoped_on(permission_set, platform),
                    "'{}' reaches a session but its {platform} resource binding does not name \
                     ${{resourceName}}; the reach scan must judge it by scope, not by its key",
                    permission_set.id
                );
            }
        }
    }

    /// The Remote Bindings platform gate refuses a kind whose set does not cover the deployment's
    /// platform. `sandbox/remote-execute` covers AWS, Azure and GCP; `alien-manager`'s resolve route
    /// carries the matching pair of arms.
    #[test]
    fn remote_binding_permission_sets_cover_the_platforms_that_support_them() {
        use alien_core::Platform;

        for id in [
            "storage/remote-data-write",
            "key/remote-cryptography",
            "ai/invoke",
        ] {
            for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
                assert!(
                    permission_set_covers_platform(id, platform),
                    "{id} must grant something on {platform}"
                );
            }
        }

        // Every cloud whose sandbox parent setup can create and then scope a grant to: an AWS
        // MicroVM image, an Azure sandbox group, a GCP reasoning engine.
        for platform in [Platform::Aws, Platform::Azure, Platform::Gcp] {
            assert!(permission_set_covers_platform(
                "sandbox/remote-execute",
                platform
            ));
        }

        for platform in [Platform::Local] {
            assert!(
                !permission_set_covers_platform("sandbox/remote-execute", platform),
                "widening sandbox/remote-execute to {platform} must be done together with \
                 alien-manager's resolve route, alien-preflights' platform gate, and \
                 permission_set_reaches_a_sandbox_session — which has no {platform} branch, so \
                 the single-tenancy gate would not see a set that reaches a session there"
            );
        }

        assert!(!permission_set_covers_platform(
            "nonexistent/permission",
            Platform::Aws
        ));
    }

    /// The wildcard branch decides whether a grant is claimed by the remote caller instead of the
    /// deployment's management identity, so a pattern it misses leaves reach on an identity a
    /// second tenant holds.
    #[test]
    fn a_wildcard_naming_microvm_anywhere_reaches_a_session() {
        for action in [
            "lambda:Foo*Microvm",
            "lambda:*Microvm",
            "lambda:RunMicrovm",
            "lambda:Run*",
            "lambda:*",
            "*",
            "lambda:runmicrovm",
            // Matches only image verbs, but a `lambda:` head prefixes every known session verb,
            // so it is claimed anyway. Over-approximating here withholds a grant; under-
            // approximating leaves one on an identity a second tenant holds.
            "lambda:*MicrovmImage",
            // IAM's single-character wildcard. Read as a literal these match no verb at all,
            // while the grants authorize `CreateMicrovmAuthToken` and `RunMicrovm`.
            "lambda:CreateMicrov?AuthToken",
            "lambda:Run?icrovm",
        ] {
            assert!(
                action_reaches_a_microvm_session(action),
                "{action} authorizes a MicroVM verb"
            );
        }

        // The image a session launches from is not the session: `sandbox/provision` and
        // `sandbox/heartbeat` hold these, and claiming them would strip a grant they need.
        for action in [
            "lambda:GetMicrovmImage",
            "lambda:GetMicrovmImage*",
            "lambda:GetMicrovmImage*Version",
            "logs:PutLogEvents",
            "s3:GetObject*",
        ] {
            assert!(
                !action_reaches_a_microvm_session(action),
                "{action} does not reach a session"
            );
        }
    }

    /// The same question the AWS wildcard test asks, on the cloud where the namespace is a
    /// permission prefix rather than an action name. A pattern this misses leaves session reach on
    /// the deployment's management identity beside a remote caller that also holds it.
    #[test]
    fn a_gcp_permission_naming_the_sandbox_namespace_reaches_a_session() {
        for permission in [
            "aiplatform.sandboxEnvironments.create",
            "aiplatform.sandboxEnvironments.delete",
            "aiplatform.sandboxEnvironments.execute",
            "aiplatform.sandboxEnvironments.pause",
            "aiplatform.sandboxEnvironments.resume",
            "aiplatform.sandboxEnvironments.snapshot",
            "aiplatform.sandboxenvironments.execute",
            // Below the namespace, and above it: both cover a verb past get/list.
            "aiplatform.sandboxEnvironments.*",
            "aiplatform.*",
            "*",
        ] {
            assert!(
                permission_reaches_a_sandbox_session(permission),
                "{permission} authorizes a sandbox session verb"
            );
        }

        // Existence and state, conferring nothing over the session or inside it. The parent's own
        // resources are not the session at all.
        for permission in [
            "aiplatform.sandboxEnvironments.get",
            "aiplatform.sandboxEnvironments.list",
            "aiplatform.sandboxEnvironmentTemplates.get",
            "aiplatform.sandboxEnvironmentTemplates.create",
            "aiplatform.reasoningEngines.create",
            "storage.objects.get",
        ] {
            assert!(
                !permission_reaches_a_sandbox_session(permission),
                "{permission} does not reach a session"
            );
        }
    }

    /// Predefined roles read off their live definitions, each carrying
    /// `aiplatform.sandboxEnvironments.execute`. A management profile naming one holds the reach
    /// the remote caller was published, so the reach scan has to see it through the role name.
    #[test]
    fn a_gcp_predefined_role_carrying_the_execute_verb_reaches_a_session() {
        use alien_core::permissions::{
            BindingConfiguration, GcpBindingSpec, GcpPlatformPermission, PermissionGrant,
        };

        let entry = |roles: &[&str]| GcpPlatformPermission {
            label: None,
            description: None,
            grant: PermissionGrant {
                predefined_roles: Some(roles.iter().map(|role| (*role).to_string()).collect()),
                ..PermissionGrant::default()
            },
            binding: BindingConfiguration::<GcpBindingSpec> {
                stack: None,
                resource: None,
            },
        };

        // Verified against the live role definitions, along with `roles/aiplatform.viewer`
        // carrying no session verb.
        for role in [
            "roles/owner",
            "roles/editor",
            "roles/aiplatform.user",
            "roles/aiplatform.admin",
            "roles/aiplatform.editor",
            "roles/aiplatform.expressAdmin",
            "roles/aiplatform.expressUser",
            "roles/aiplatform.customCodeServiceAgent",
            "roles/aiplatform.serviceAgent",
            "roles/discoveryengine.serviceAgent",
            // Neither enumerable: a role Vertex AI adds later, and a custom role of any name.
            "roles/aiplatform.someRoleAddedLater",
            "projects/example/roles/aCustomRole",
        ] {
            assert!(
                gcp_entry_reaches_a_sandbox_session(&entry(&[role])),
                "{role} must be treated as reaching a sandbox session"
            );
        }
        // A neighbouring resource's grant must not refuse a deployment: these are the GCP roles
        // shipped permission sets actually name, and none of them reaches a sandbox session.
        for role in [
            "roles/aiplatform.viewer",
            "roles/datastore.viewer",
            "roles/datastore.user",
            "roles/storage.objectAdmin",
            "roles/storage.bucketViewer",
            "roles/cloudkms.viewer",
            "roles/cloudkms.cryptoKeyEncrypterDecrypter",
            "roles/artifactregistry.reader",
        ] {
            assert!(
                !gcp_entry_reaches_a_sandbox_session(&entry(&[role])),
                "{role} carries no session verb"
            );
        }
    }

    #[test]
    fn test_permission_set_structure() {
        let function_exec = get_permission_set("worker/execute").unwrap();

        // Test AWS platform
        if let Some(aws_perms) = &function_exec.platforms.aws {
            assert!(!aws_perms.is_empty());
            let first_perm = &aws_perms[0];

            // Should have actions
            assert!(first_perm.grant.actions.is_some());
            let actions = first_perm.grant.actions.as_ref().unwrap();
            assert!(actions.contains(&"logs:PutLogEvents".to_string()));

            // Should have bindings
            assert!(!first_perm.binding.is_empty());
            assert!(first_perm.binding.stack.is_some());
            assert!(first_perm.binding.resource.is_some());
        }
    }
}

/// The global management refs this deployment's own identity keeps.
///
/// A set the remote caller claims is dropped. Leaving the same reach on the management identity
/// makes it the second tenant the single-tenancy gate exists to refuse, and the gate runs at plan
/// time while this runs on every apply and every update — so setup and runtime have to answer the
/// same way or the first update re-grants what the package withheld.
pub fn management_identity_global_refs<'p, 'r, I>(
    resources: I,
    profile: &'p alien_core::permissions::PermissionProfile,
) -> Vec<&'p alien_core::permissions::PermissionSetReference>
where
    I: IntoIterator<Item = &'r alien_core::ResourceEntry> + Clone,
{
    profile
        .0
        .get("*")
        .map(|refs| {
            refs.iter()
                .filter(|permission_ref| {
                    !alien_core::remote_bindings::remote_binding_claims_management_set(
                        resources.clone(),
                        permission_ref.id(),
                        || {
                            permission_ref
                                .resolve(|name| get_permission_set(name).cloned())
                                .is_some_and(|set| permission_set_reaches_a_sandbox_session(&set))
                        },
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}
