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

/// Whether `permission_set` grants anything that addresses a sandbox session, on any cloud that
/// publishes one.
///
/// Bindings are deliberately not consulted. `${stackPrefix}` is uninterpolated this early and an
/// inline set's ARNs are free-form user strings, so any ARN comparison is either unsound or
/// widened past by writing `*`. Carrying the verb at all is the answer.
///
/// Per-cloud rather than per-verb-list, because the verbs are not comparable: AWS names actions,
/// Azure grants the same reach through a predefined role or through `dataActions`. A set is
/// inspected on every cloud it declares — a caller asking "can anything else here reach this
/// sandbox" is asking about the deployment, and a set carrying an Azure block alone would
/// otherwise answer no while granting the whole data plane.
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
        .any(|entry| {
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
        });

    reaches_on_aws || reaches_on_azure
}

/// The predefined role carrying Azure's whole sandbox data plane, session contents included.
///
/// Public because three places have to agree on it — this predicate, the invariant test that keeps
/// it out of the implicit-management sets, and the allowlist of roles an author may name. A second
/// sandbox data-plane role added to that allowlist and not here drops silently out of both checks.
pub const AZURE_SANDBOX_DATA_PLANE_ROLE: &str = "Container Apps SandboxGroup Data Owner";

/// Whether one Azure `dataAction`, possibly carrying a `*`, addresses a sandbox session.
///
/// Lifecycle counts as reach for the same reason `RunMicrovm` does on AWS: whoever starts a
/// session can put whatever it likes inside the one it started.
///
/// A wildcard is compared as a prefix in **both** directions, which is what makes it fail closed:
/// `*` and `Microsoft.App/*` sit above the sandbox namespace and still reach into it, while
/// `…/sandboxes/*` sits below. Matching only downwards would clear the two that matter most. Every
/// verb under the namespace counts rather than a suffix allowlist, so a verb Azure adds later is
/// reach until someone decides otherwise.
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
/// Three cases sit together here: a wildcard is cleared against the known verbs *and* the MicroVM
/// namespace, an exact action is matched on that namespace so a verb AWS adds later fails closed,
/// and the `MicrovmImage` family is excluded because it addresses the image a session launches
/// from — `sandbox/provision` and `sandbox/heartbeat` legitimately hold those.
///
/// The namespace half of the wildcard case is what keeps the two branches agreeing: clearing a
/// wildcard against today's names alone would answer no for `lambda:SomeFutureMicrovmVerb*` while
/// the un-wildcarded form answers yes, so the broader grant would be the one that slipped through.
///
/// Compared lowercased throughout, because AWS matches action names case-insensitively — a set
/// granting `lambda:runmicrovm` reaches a session exactly as `lambda:RunMicrovm` does.
fn action_reaches_a_microvm_session(action: &str) -> bool {
    let action = action.to_ascii_lowercase();
    if action.contains('*') {
        let literal = action.split('*').next().unwrap_or_default();
        let covers_a_known_verb = SENSITIVE_MICROVM_ACTIONS
            .iter()
            .chain(MICROVM_SESSION_LIFECYCLE_ACTIONS)
            .any(|known| known.to_ascii_lowercase().starts_with(literal));
        // Every literal run, not only the one before the first `*`: `lambda:Foo*Microvm` names
        // the namespace after the wildcard, and reading the head alone would answer no while the
        // grant authorizes a MicroVM verb. Each run is cleared the same way, so the `MicrovmImage`
        // exclusion still applies to whichever run carries the name.
        return covers_a_known_verb
            || action.split('*').any(reaches_the_microvm_namespace);
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

    /// The Remote Bindings platform gate refuses a kind whose set does not cover the deployment's
    /// platform, so this data is what decides where each kind may be published.
    /// `sandbox/remote-execute` covers AWS and Azure; `alien-manager`'s resolve route carries the
    /// matching pair of arms.
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
    /// deployment's management identity, so a shape it answers no to stays on an identity a second
    /// tenant holds. Reading only the run before the first `*` answered no to a pattern naming the
    /// namespace after it.
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
