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

    reaches_on_aws || reaches_on_azure
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

/// Whether the session-reaching part of a set is scoped to the resource it is filed under.
///
/// The single-tenancy gate treats a **named** set as scoped by the profile key it sits under. That
/// only holds where the entries carrying the session-reaching grant interpolate `${resourceName}`:
/// an entry scoped to a whole project or subscription reaches every sibling whatever key it is
/// filed under, and has to be judged by its scope instead. Entries that reach no session are not
/// consulted — `sandbox/remote-execute` binds AWS's own network connector by a fixed ARN, which
/// names no sandbox and grants nothing inside one.
pub fn permission_set_is_resource_scoped_on(
    permission_set: &alien_core::permissions::PermissionSet,
    platform: alien_core::Platform,
) -> bool {
    const TOKEN: &str = "${resourceName}";
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
                        .any(|resource| resource.contains(TOKEN))
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
                    .is_some_and(|spec| spec.scope.contains(TOKEN))
            }),
        alien_core::Platform::Gcp
        | alien_core::Platform::Kubernetes
        | alien_core::Platform::Machines
        | alien_core::Platform::Local
        | alien_core::Platform::Test => true,
    }
}

#[cfg(test)]
mod tests {
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

    /// The single-tenancy gate scopes a **named** session-reaching set by the profile key it sits
    /// under, which only holds while every such set's resource binding names `${resourceName}`.
    /// A set that reaches a session through a project- or subscription-wide resource binding has
    /// to fall through to the inline treatment instead, so this pins the assumption at the source.
    #[test]
    fn every_session_reaching_set_is_resource_scoped_by_resource_name() {
        for id in list_permission_set_ids() {
            let permission_set = get_permission_set(id).expect("a listed set resolves");
            if !permission_set_reaches_a_sandbox_session(permission_set) {
                continue;
            }
            for platform in [
                alien_core::Platform::Aws,
                alien_core::Platform::Gcp,
                alien_core::Platform::Azure,
            ] {
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
    /// platform. `sandbox/remote-execute` covers AWS and Azure; `alien-manager`'s resolve route
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

        // Both clouds whose sandbox parent setup can create and then scope a grant to: an AWS
        // MicroVM image, and an Azure sandbox group.
        for platform in [Platform::Aws, Platform::Azure] {
            assert!(permission_set_covers_platform(
                "sandbox/remote-execute",
                platform
            ));
        }

        for platform in [Platform::Gcp, Platform::Local] {
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
