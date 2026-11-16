//! Scan for the product name in text an end customer's administrator reads.
//!
//! The product is sold white-labeled: what a vendor ships to its customers must not name the
//! platform underneath. Generators render CloudFormation, Terraform and Helm; their tests pass
//! every rendered file through [`assert_white_labeled`].
//!
//! A mention is the product name in any case, so also the project's domain and organisation
//! names. Text that only a running workload reads, and never the administrator, is listed in
//! [`ALLOWLIST`] with the reason it is allowed. Anything else fails the scan.
//!
//! The Terraform crate's AWS white-label test keeps its own ledger of exact occurrences in the
//! setup and runtime policies it renders, which may only shrink. This scan is a pattern list
//! for every generator scenario; the two answer different questions and are not merged.

/// The text that counts as a mention, compared in lower case.
const NEEDLE: &str = "alien";

/// One entry of a written list. It covers a mention when the whole token the mention sits in
/// contains `token_contains` and, if given, the line contains `line_contains`.
pub struct Known {
    pub token_contains: &'static str,
    pub line_contains: Option<&'static str>,
    pub reason: &'static str,
}

impl Known {
    fn covers(&self, token: &str, line: &str) -> bool {
        let token_matches = if self.token_contains == LABEL_KEY {
            is_label_key(token)
        } else if let Some(exact) = self.token_contains.strip_prefix('=') {
            // A leading `=` means the whole token must equal the text, not contain it.
            token == exact
        } else if self.token_contains.contains(['=', ':', '/']) {
            token.contains(self.token_contains)
        } else {
            // `KEY=https://host.example` is several words: every word that names the product
            // must itself hold the entry's text, so a hostname after `=` is not covered by
            // the variable name in front of it.
            token
                .split(['=', ':', '/'])
                .filter(|part| part.to_ascii_lowercase().contains(NEEDLE))
                .all(|part| part.contains(self.token_contains))
        };
        token_matches && self.line_contains.is_none_or(|text| line.contains(text))
    }
}

/// Stands for "a Kubernetes label or annotation key in the project's domain", written
/// `alien.dev/<name>` or `setup.alien.dev/<name>`, and not a hostname or URL that ends in it.
const LABEL_KEY: &str = "alien.dev/";

fn is_label_key(token: &str) -> bool {
    // A jsonpath step escapes the dot inside the key; a hostname never does.
    if token.contains("alien\\.dev/") {
        return true;
    }
    let escaped = token.replace("\\.", ".");
    escaped.match_indices(LABEL_KEY).any(|(index, _)| {
        let before = &escaped[..index];
        let before = before.strip_suffix("setup.").unwrap_or(before);
        !before.ends_with(|c: char| c.is_ascii_alphanumeric() || c == '-' || c == '.')
            && !before.ends_with("://")
    })
}

/// What may mention the product, each with the reason. Adding an entry is a product decision.
pub const ALLOWLIST: &[Known] = &[
    Known {
        token_contains: "ALIEN_",
        line_contains: None,
        reason: "environment variable names read by the workload inside its container; the \
                 administrator reviews the setup, not the process environment",
    },
    Known {
        token_contains: "alien-runtime-cleanup",
        line_contains: None,
        reason: "scratch directory inside the cleanup job's pod; never visible outside the job",
    },
];

/// Mentions that are fixed only by renaming something that exists on installed deployments (a
/// label, annotation or physical name that code looks up). Renaming would replace resources or
/// strand upgrades, so these stay until a follow-up changes them for new installs. Each entry is
/// a known gap, not an approval: [`find_pending_renames`] lists them for the scan report as
/// "rename-follow-up".
pub const PENDING_RENAMES: &[Known] = &[
    Known {
        token_contains: LABEL_KEY,
        line_contains: None,
        reason: "label, annotation and ConfigMap-key names that the chart, the Operator and the \
                 cleanup jobs select on and compare on installed clusters (includes the \
                 `setup.` fingerprint annotations and the Postgres image annotation the chart \
                 reads back)",
    },
    Known {
        token_contains: "serviceaccount:alien:",
        line_contains: None,
        reason: "the managed cluster's default Kubernetes namespace, named in the identity trust \
                 conditions of existing roles; changing it breaks workload identity",
    },
    Known {
        token_contains: "=alien",
        line_contains: Some("namespace:"),
        reason: "the managed cluster's default Kubernetes namespace (see the trust conditions)",
    },
    Known {
        token_contains: "=alien",
        line_contains: Some("\"namespace\":\"alien\""),
        reason: "the managed cluster's default Kubernetes namespace in the JSON form of the trust \
                 conditions (see above)",
    },
    Known {
        token_contains: "alien-sbx-",
        line_contains: None,
        reason: "physical name of the sandbox network policy on installed clusters; the chart \
                 replaces it on upgrade and the Operator's cleanup selects it",
    },
    Known {
        token_contains: "alien-dc-",
        line_contains: None,
        reason: "physical name of the remote Operator's resources on existing installs",
    },
    Known {
        token_contains: "/opt/alien",
        line_contains: None,
        reason: "install directory inside the sandbox image that the runtime's protected mounts \
                 name; sandbox images already built mount it by this path",
    },
    Known {
        token_contains: "alienplatform/alien-egress-gateway",
        line_contains: None,
        reason: "the egress gateway image is pulled from the product's registry and pinned by \
                 digest; showing a vendor registry needs the image published there, which is a \
                 release-process change",
    },
    Known {
        token_contains: "alien-sandbox-",
        line_contains: None,
        reason: "physical name of a sandbox capability secret that the cleanup job reads",
    },
    Known {
        token_contains: "alien",
        line_contains: Some("coalesce(trim(lower(replace("),
        reason: "fallback of the Kubernetes service-account name, which must match the name \
                 the runtime derives for the same account",
    },
    Known {
        token_contains: "alien",
        line_contains: Some("default \"alien\" $collapsed"),
        reason: "fallback name prefix for chart resources on installs that predate named releases",
    },
    Known {
        token_contains: "alien",
        line_contains: Some("alien build placeholder"),
        reason: "placeholder step of an Azure build task; changing it plans an in-place update \
                 on every existing install",
    },
];

/// One mention outside the allowlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mention {
    pub artifact: String,
    /// 1-based line number.
    pub line: usize,
    /// The whole token the mention sits in.
    pub token: String,
    /// The line it sits on, trimmed.
    pub context: String,
}

impl std::fmt::Display for Mention {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}: [{}] {}",
            self.artifact, self.line, self.token, self.context
        )
    }
}

fn is_delimiter(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '"' | '\'' | '`' | ',' | '(' | ')' | '[' | ']' | '{' | '}'
        )
}

fn scan(artifact: &str, content: &str, keep: impl Fn(&str, &str) -> bool) -> Vec<Mention> {
    let mut mentions = Vec::new();
    for (index, line) in content.lines().enumerate() {
        for token in line.split(is_delimiter) {
            if !token.to_ascii_lowercase().contains(NEEDLE) || !keep(token, line) {
                continue;
            }
            mentions.push(Mention {
                artifact: artifact.to_string(),
                line: index + 1,
                token: token.to_string(),
                context: line.trim().chars().take(160).collect(),
            });
        }
    }
    mentions
}

/// Every mention in `content` that neither the allowlist nor the pending renames cover.
pub fn find_mentions(artifact: &str, content: &str) -> Vec<Mention> {
    scan(artifact, content, |token, line| {
        !ALLOWLIST
            .iter()
            .chain(PENDING_RENAMES)
            .any(|known| known.covers(token, line))
    })
}

/// The mentions in `content` that only [`PENDING_RENAMES`] covers: the "rename-follow-up" rows of
/// the scan report. The generator tests do not fail on them.
pub fn find_pending_renames(artifact: &str, content: &str) -> Vec<Mention> {
    scan(artifact, content, |token, line| {
        !ALLOWLIST.iter().any(|known| known.covers(token, line))
            && PENDING_RENAMES
                .iter()
                .any(|known| known.covers(token, line))
    })
}

/// Panic, listing every mention, when `content` names the product outside the allowlist.
pub fn assert_white_labeled(artifact: &str, content: &str) {
    assert_files_white_labeled(artifact, [(artifact, content)]);
}

/// [`assert_white_labeled`] for the files of one artifact: reports the mentions of all files,
/// not only the first file that has one.
pub fn assert_files_white_labeled<'a>(
    artifact: &str,
    files: impl IntoIterator<Item = (&'a str, &'a str)>,
) {
    let mentions: Vec<Mention> = files
        .into_iter()
        .flat_map(|(path, content)| find_mentions(&format!("{artifact}/{path}"), content))
        .collect();
    assert!(
        mentions.is_empty(),
        "generated artifact names the product ({} mention(s)):\n{}",
        mentions.len(),
        mentions
            .iter()
            .map(Mention::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_names_domains_and_tags() {
        assert_eq!(find_mentions("a", "Description: Alien setup").len(), 1);
        assert_eq!(find_mentions("a", "url: https://x.alien.dev/p").len(), 1);
        assert_eq!(find_mentions("a", "Tag: alien").len(), 1);
    }

    #[test]
    fn flags_a_label_key_once_it_is_not_a_known_rename() {
        // `alien.dev/` keys are known renames; a product name in a description is not.
        assert!(find_mentions("a", "Key: alien.dev/deployment").is_empty());
        assert_eq!(find_mentions("a", "Description: Alien setup").len(), 1);
        assert_eq!(find_mentions("a", "host: manager.alien.dev/x").len(), 1);
        assert!(find_mentions("a", "jsonpath={.metadata.labels.alien\\.dev/sandbox}").is_empty());
    }

    #[test]
    fn allows_container_env_var_names() {
        assert!(find_mentions("a", "- name: ALIEN_MANAGER_AUDIENCE").is_empty());
    }

    #[test]
    fn clean_text_passes() {
        assert!(find_mentions("a", "Description: Acme setup").is_empty());
    }

    #[test]
    fn namespace_exception_is_the_exact_token() {
        assert!(find_mentions("a", "namespace: alien").is_empty());
        assert_eq!(
            find_mentions("a", "description: alien operator namespace").len(),
            1
        );
        assert_eq!(find_mentions("a", "namespace: alien-extra").len(), 1);
    }

    #[test]
    fn namespace_exception_covers_the_json_form_only_exactly() {
        assert!(find_mentions("a", "\"namespace\":\"alien\"").is_empty());
        assert_eq!(find_mentions("a", "\"namespace\":\"alien-x\"").len(), 1);
        assert_eq!(find_mentions("a", "\"description\":\"alien\"").len(), 1);
    }

    #[test]
    fn env_var_name_does_not_cover_a_hostname_after_it() {
        assert!(find_mentions("a", "- name: ALIEN_MANAGER_URL").is_empty());
        assert!(find_mentions("a", "ALIEN_MANAGER_URL=https://example.test").is_empty());
        assert_eq!(
            find_mentions("a", "ALIEN_MANAGER_URL=https://host.alien.example").len(),
            1
        );
    }

    #[test]
    fn pending_renames_are_reported_separately() {
        let line = "app.kubernetes.io/part-of: x\n  alien.dev/sandbox: \"1\"";
        assert!(find_mentions("a", line).is_empty());
        assert_eq!(find_pending_renames("a", line).len(), 1);
        assert!(find_pending_renames("a", "Description: Alien setup").is_empty());
    }
}
