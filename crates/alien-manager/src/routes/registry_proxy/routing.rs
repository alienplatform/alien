use super::*;

// ---------------------------------------------------------------------------
// Registry routing table
// ---------------------------------------------------------------------------

/// A route mapping a repository path prefix to an artifact registry provider.
#[derive(Clone)]
pub struct RegistryRoute {
    pub prefix: String,
    pub platform: Platform,
    pub provider: Arc<dyn BindingsProviderApi>,
    pub binding_name: String,
}

/// Routes OCI requests to the correct upstream registry based on repo path prefix.
/// Built once at startup from the manager's artifact registry configuration.
pub struct RegistryRoutingTable {
    /// Routes sorted by prefix length descending (longest prefix match wins).
    routes: Vec<RegistryRoute>,
}

impl RegistryRoutingTable {
    pub fn new(mut routes: Vec<RegistryRoute>) -> Result<Self, String> {
        Self::validate_unique_prefixes(&routes)?;
        // Sort by prefix length descending for longest-prefix match.
        routes.sort_by(|a, b| b.prefix.len().cmp(&a.prefix.len()));
        Ok(Self { routes })
    }

    /// Find the registry route that matches the given repo name.
    pub fn resolve(&self, repo_name: &str) -> Option<&RegistryRoute> {
        self.routes.iter().find(|r| {
            if r.prefix.is_empty() {
                // Empty prefix = catch-all; construction rejects more than one.
                true
            } else {
                repo_name.starts_with(&r.prefix)
            }
        })
    }

    /// Extract the project_id from an OCI repo path using this table's
    /// boot-time-static `prefix → platform` map. The provider that owns the
    /// matching route composes its full repo name as `{prefix}{sep}{name}` —
    /// `-` for ECR, `/` for GAR/ACR/Local — so we strip the prefix, strip the
    /// single separator byte, and take everything up to the next `/` as the
    /// project_id.
    ///
    /// Returns `None` when no route matches, when the suffix doesn't start
    /// with `-` or `/` (defense — a path that didn't go through a provider's
    /// `make_full_repo_name`), or when the extracted id is empty. Callers
    /// fall back to `"default"`; the [`crate::auth::Authz`] impl then
    /// decides whether to allow the push.
    pub fn project_id_for_repo<'a>(&self, repo_name: &'a str) -> Option<&'a str> {
        let route = self.resolve(repo_name)?;
        project_id_after_prefix(repo_name, route.prefix.as_str())
    }

    /// Get the repo prefix for a given platform.
    pub fn prefix_for_platform(&self, platform: Platform) -> Option<&str> {
        self.routes
            .iter()
            .find(|r| r.platform == platform)
            .map(|r| r.prefix.as_str())
    }

    /// Return the list of explicitly configured (non-fallback) platforms.
    ///
    /// These are cloud platforms with dedicated artifact registries (ECR, GAR, ACR).
    /// The local catch-all fallback is excluded.
    pub fn configured_platforms(&self) -> Vec<Platform> {
        let mut platforms: Vec<Platform> = self
            .routes
            .iter()
            .filter(|r| r.platform != Platform::Local)
            .map(|r| r.platform)
            .collect();
        platforms.dedup();
        platforms
    }

    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }

    /// Validate no ambiguous prefixes (call at startup).
    pub fn validate(&self) -> Result<(), String> {
        Self::validate_unique_prefixes(&self.routes)
    }

    fn validate_unique_prefixes(routes: &[RegistryRoute]) -> Result<(), String> {
        let mut seen: HashMap<&str, &RegistryRoute> = HashMap::new();
        for route in routes {
            if let Some(existing) = seen.insert(route.prefix.as_str(), route) {
                let prefix = if route.prefix.is_empty() {
                    "<empty>"
                } else {
                    route.prefix.as_str()
                };
                return Err(format!(
                    "Duplicate artifact registry prefix '{}' for {} binding '{}' and {} binding '{}'",
                    prefix,
                    existing.platform,
                    existing.binding_name,
                    route.platform,
                    route.binding_name
                ));
            }
        }
        Ok(())
    }
}

/// Strip `prefix` from `repo_name`, then strip a single separator byte
/// (`-` for ECR, `/` for GAR/ACR/Local — empty prefix needs no separator),
/// and return everything up to the next `/`. See
/// [`RegistryRoutingTable::project_id_for_repo`] for the full algorithm
/// (including the route resolution this helper assumes has already happened).
///
/// Exposed at module level so unit tests can exercise the algorithm without
/// constructing a full `RegistryRoutingTable` (which would require a real
/// `BindingsProviderApi` and a tokio runtime).
pub(super) fn project_id_after_prefix<'a>(repo_name: &'a str, prefix: &str) -> Option<&'a str> {
    let suffix = if prefix.is_empty() {
        repo_name
    } else {
        let rest = repo_name.strip_prefix(prefix)?;
        let first = rest.chars().next()?;
        if first != '-' && first != '/' {
            return None;
        }
        &rest[1..]
    };
    let pid = suffix.split('/').next()?;
    if pid.is_empty() {
        None
    } else {
        Some(pid)
    }
}
