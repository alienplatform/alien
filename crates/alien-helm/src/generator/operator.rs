use super::{
    ensure_trailing_newline, sanitize_chart_name, validate_runtime_encryption_key, yaml_key,
    yaml_string, OperatorManifestOptions, OperatorOutputFormat,
};
use alien_core::{ErrorData, Result};
use alien_error::AlienError;
use std::collections::BTreeMap;

pub fn generate_operator_manifest(options: OperatorManifestOptions<'_>) -> Result<String> {
    validate_runtime_encryption_key(options.encryption_key)?;
    validate_operator_options(&options)?;

    let base_name = sanitize_chart_name(options.project_name);
    let operator_name = format!("{base_name}-operator");
    let identity_pvc_name = format!("{operator_name}-identity");

    // The install namespace: where every operator object lives, the binding
    // subject's namespace, and (in `Namespace` scope) the observed namespace. A
    // Helm template defers it to `.Release.Namespace`; a raw manifest pins it to
    // a concrete value so `kubectl apply` and binding subjects resolve.
    let namespace_expr = match options.format {
        OperatorOutputFormat::HelmTemplate => "{{ .Release.Namespace }}".to_string(),
        OperatorOutputFormat::RawManifest => options
            .install_namespace
            .expect("validated by validate_operator_options")
            .to_string(),
    };
    let namespace = namespace_expr.as_str();

    // OPERATOR_NAME is the per-environment identity. A raw manifest carries the
    // concrete name; a Helm template sources it per install from values so one
    // file registers every customer environment distinctly.
    let environment_name_expr = match options.format {
        OperatorOutputFormat::HelmTemplate => "{{ .Values.alien.environmentName }}".to_string(),
        OperatorOutputFormat::RawManifest => options
            .environment_name
            .expect("validated by validate_operator_options")
            .to_string(),
    };

    let labels = operator_labels(&base_name);
    let cluster_wide = options.scope.is_cluster_wide();

    let mut docs = Vec::new();
    docs.push(operator_service_account_doc(
        namespace,
        &operator_name,
        &labels,
    ));
    // Cluster-wide (label) scope needs cluster-scoped read RBAC; namespace scope
    // stays a namespaced Role. Both grant only get/list/watch.
    if cluster_wide {
        docs.push(operator_clusterrole_doc(&operator_name, &labels));
        docs.push(operator_clusterrolebinding_doc(
            namespace,
            &operator_name,
            &labels,
        ));
    } else {
        docs.push(operator_role_doc(namespace, &operator_name, &labels));
        docs.push(operator_rolebinding_doc(namespace, &operator_name, &labels));
    }
    docs.push(operator_secret_doc(
        namespace,
        &operator_name,
        options.group_token,
        options.encryption_key,
        options
            .log_collector
            .as_ref()
            .map(|collector| collector.token),
        &labels,
    ));
    docs.push(operator_identity_pvc_doc(
        namespace,
        &identity_pvc_name,
        &labels,
    ));
    docs.push(operator_deployment_doc(
        namespace,
        &operator_name,
        &identity_pvc_name,
        &options,
        namespace,
        &environment_name_expr,
        options.label_selector,
        &labels,
    ));
    if let Some(log_collector) = options.log_collector.as_ref() {
        let mut collector_labels = labels.clone();
        collector_labels.insert(
            "app.kubernetes.io/component".to_string(),
            "whitelabeled-log-collector".to_string(),
        );
        docs.push(operator_service_doc(namespace, &operator_name, &labels));
        docs.push(operator_log_collector_service_account_doc(
            namespace,
            &operator_name,
            &collector_labels,
        ));
        docs.push(operator_log_collector_role_doc(
            namespace,
            &operator_name,
            &collector_labels,
        ));
        docs.push(operator_log_collector_role_binding_doc(
            namespace,
            &operator_name,
            &collector_labels,
        ));
        docs.push(operator_log_collector_configmap_doc(
            namespace,
            &operator_name,
            namespace,
            &collector_labels,
        ));
        docs.push(operator_log_collector_daemonset_doc(
            namespace,
            &operator_name,
            log_collector.image,
            &collector_labels,
        ));
    }

    Ok(ensure_trailing_newline(docs.join("---\n")))
}

fn validate_operator_options(options: &OperatorManifestOptions<'_>) -> Result<()> {
    let invalid = |message: &str| {
        Err(AlienError::new(ErrorData::GenericError {
            message: message.to_string(),
        }))
    };

    // A label selector, when given, must be non-empty.
    if let Some(selector) = options.label_selector {
        if selector.trim().is_empty() {
            return invalid("operator label selector must not be empty");
        }
    }

    // Raw manifests are applied to one concrete cluster, so the install namespace
    // and per-environment identity must be concrete. Helm defers both to install.
    if options.format == OperatorOutputFormat::RawManifest {
        if options
            .install_namespace
            .map(|ns| ns.trim().is_empty())
            .unwrap_or(true)
        {
            return invalid("raw manifests require an install namespace");
        }
        if options
            .environment_name
            .map(|name| name.trim().is_empty())
            .unwrap_or(true)
        {
            return invalid("raw manifests require an environment name");
        }
    }

    Ok(())
}

fn operator_labels(base_name: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("app.kubernetes.io/name".to_string(), "operator".to_string()),
        (
            "app.kubernetes.io/instance".to_string(),
            base_name.to_string(),
        ),
        (
            "app.kubernetes.io/component".to_string(),
            "operator".to_string(),
        ),
        (
            "app.kubernetes.io/managed-by".to_string(),
            "kubectl".to_string(),
        ),
    ])
}

fn operator_service_account_doc(
    namespace: &str,
    operator_name: &str,
    labels: &BTreeMap<String, String>,
) -> String {
    let mut yaml = operator_metadata_doc("v1", "ServiceAccount", namespace, operator_name, labels);
    yaml.push_str("automountServiceAccountToken: true\n");
    yaml
}

fn operator_role_doc(
    namespace: &str,
    operator_name: &str,
    labels: &BTreeMap<String, String>,
) -> String {
    let mut yaml = operator_metadata_doc(
        "rbac.authorization.k8s.io/v1",
        "Role",
        namespace,
        operator_name,
        labels,
    );
    yaml.push_str(OPERATOR_OBSERVE_RULES);
    yaml
}

fn operator_rolebinding_doc(
    namespace: &str,
    operator_name: &str,
    labels: &BTreeMap<String, String>,
) -> String {
    let mut yaml = operator_metadata_doc(
        "rbac.authorization.k8s.io/v1",
        "RoleBinding",
        namespace,
        operator_name,
        labels,
    );
    yaml.push_str(&format!(
        r#"subjects:
  - kind: ServiceAccount
    name: {}
    namespace: {}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: Role
  name: {}
"#,
        yaml_string(operator_name),
        yaml_string(namespace),
        yaml_string(operator_name)
    ));
    yaml
}

/// Read-only observe rules, shared by the namespaced `Role` and the cluster-wide
/// `ClusterRole`. Only `get/list/watch`; never `secrets` or `pods/log`.
const OPERATOR_OBSERVE_RULES: &str = r#"rules:
  - apiGroups: [""]
    resources: ["pods", "services", "configmaps", "persistentvolumeclaims", "events", "endpoints"]
    verbs: ["get", "list", "watch"]
  - apiGroups: ["apps"]
    resources: ["deployments", "statefulsets", "daemonsets", "replicasets"]
    verbs: ["get", "list", "watch"]
  - apiGroups: ["batch"]
    resources: ["jobs", "cronjobs"]
    verbs: ["get", "list", "watch"]
  - apiGroups: ["metrics.k8s.io"]
    resources: ["pods"]
    verbs: ["get", "list", "watch"]
"#;

/// Cluster-scoped metadata header (no `metadata.namespace`) for `ClusterRole` and
/// `ClusterRoleBinding`, which are not namespaced objects.
fn operator_cluster_metadata_doc(
    kind: &str,
    name: &str,
    labels: &BTreeMap<String, String>,
) -> String {
    let mut yaml = String::new();
    yaml.push_str("apiVersion: rbac.authorization.k8s.io/v1\n");
    yaml.push_str(&format!("kind: {}\n", yaml_string(kind)));
    yaml.push_str("metadata:\n");
    yaml.push_str(&format!("  name: {}\n", yaml_string(name)));
    yaml.push_str("  labels:\n");
    append_operator_labels(&mut yaml, labels, 4);
    yaml
}

fn operator_clusterrole_doc(operator_name: &str, labels: &BTreeMap<String, String>) -> String {
    let mut yaml = operator_cluster_metadata_doc("ClusterRole", operator_name, labels);
    yaml.push_str(OPERATOR_OBSERVE_RULES);
    yaml
}

fn operator_clusterrolebinding_doc(
    subject_namespace: &str,
    operator_name: &str,
    labels: &BTreeMap<String, String>,
) -> String {
    let mut yaml = operator_cluster_metadata_doc("ClusterRoleBinding", operator_name, labels);
    yaml.push_str(&format!(
        r#"subjects:
  - kind: ServiceAccount
    name: {}
    namespace: {}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: {}
"#,
        yaml_string(operator_name),
        yaml_string(subject_namespace),
        yaml_string(operator_name)
    ));
    yaml
}

fn operator_secret_doc(
    namespace: &str,
    operator_name: &str,
    group_token: &str,
    encryption_key: &str,
    collector_token: Option<&str>,
    labels: &BTreeMap<String, String>,
) -> String {
    let mut yaml = operator_metadata_doc("v1", "Secret", namespace, operator_name, labels);
    yaml.push_str("type: Opaque\n");
    yaml.push_str("stringData:\n");
    yaml.push_str(&format!("  sync-token: {}\n", yaml_string(group_token)));
    yaml.push_str(&format!(
        "  encryption-key: {}\n",
        yaml_string(encryption_key)
    ));
    if let Some(collector_token) = collector_token {
        yaml.push_str(&format!(
            "  collector-token: {}\n",
            yaml_string(collector_token)
        ));
    }
    yaml
}

fn operator_identity_pvc_doc(
    namespace: &str,
    pvc_name: &str,
    labels: &BTreeMap<String, String>,
) -> String {
    let mut yaml =
        operator_metadata_doc("v1", "PersistentVolumeClaim", namespace, pvc_name, labels);
    yaml.push_str(
        r#"spec:
  accessModes: ["ReadWriteOnce"]
  resources:
    requests:
      storage: "1Gi"
"#,
    );
    yaml
}

#[allow(clippy::too_many_arguments)]
fn operator_deployment_doc(
    namespace: &str,
    operator_name: &str,
    identity_pvc_name: &str,
    options: &OperatorManifestOptions<'_>,
    observed_namespace: &str,
    environment_name: &str,
    label_selector: Option<&str>,
    labels: &BTreeMap<String, String>,
) -> String {
    let mut yaml = operator_metadata_doc("apps/v1", "Deployment", namespace, operator_name, labels);
    yaml.push_str("spec:\n");
    yaml.push_str("  replicas: 1\n");
    yaml.push_str("  selector:\n");
    yaml.push_str("    matchLabels:\n");
    append_operator_selector_labels(&mut yaml, labels, 6);
    yaml.push_str("  template:\n");
    yaml.push_str("    metadata:\n");
    yaml.push_str("      labels:\n");
    append_operator_labels(&mut yaml, labels, 8);
    yaml.push_str("    spec:\n");
    yaml.push_str(&format!(
        "      serviceAccountName: {}\n",
        yaml_string(operator_name)
    ));
    yaml.push_str("      automountServiceAccountToken: true\n");
    yaml.push_str("      securityContext:\n");
    yaml.push_str("        runAsNonRoot: true\n");
    yaml.push_str("        runAsUser: 1000\n");
    yaml.push_str("        runAsGroup: 1000\n");
    yaml.push_str("        fsGroup: 1000\n");
    yaml.push_str("        seccompProfile:\n");
    yaml.push_str("          type: RuntimeDefault\n");
    yaml.push_str("      containers:\n");
    yaml.push_str("        - name: operator\n");
    yaml.push_str(&format!(
        "          image: {}\n",
        yaml_string(options.image)
    ));
    yaml.push_str("          imagePullPolicy: IfNotPresent\n");
    yaml.push_str("          securityContext:\n");
    yaml.push_str("            allowPrivilegeEscalation: false\n");
    yaml.push_str("            readOnlyRootFilesystem: true\n");
    yaml.push_str("            capabilities:\n");
    yaml.push_str("              drop: [\"ALL\"]\n");
    yaml.push_str("          env:\n");
    append_env_value(&mut yaml, "PLATFORM", "kubernetes");
    append_env_value(&mut yaml, "SYNC_URL", options.manager_url);
    append_env_value(&mut yaml, "OPERATOR_NAME", environment_name);
    append_env_value(&mut yaml, "KUBERNETES_NAMESPACE", namespace);
    append_env_value(&mut yaml, "OPERATOR_SCOPE", observed_namespace);
    // Cluster scope observes every namespace; namespace scope stays in its own.
    // The selector (if any) filters within whichever scope is chosen.
    if options.scope.is_cluster_wide() {
        append_env_value(&mut yaml, "OPERATOR_OBSERVE_ALL_NAMESPACES", "true");
    }
    if let Some(label_selector) = label_selector {
        append_env_value(&mut yaml, "OPERATOR_LABEL_SELECTOR", label_selector);
    }
    // Helm distributions surface the running app version as a value, so each install
    // reports the release it's on. Raw manifests omit it; the vendor sets
    // OPERATOR_RELEASE_VERSION themselves if they want version/rollout visibility.
    if options.format == OperatorOutputFormat::HelmTemplate {
        append_env_value(
            &mut yaml,
            "OPERATOR_RELEASE_VERSION",
            "{{ .Values.alien.version }}",
        );
    }
    append_env_value(
        &mut yaml,
        "OPERATOR_PERMISSION",
        options.permission.as_str(),
    );
    append_env_value(&mut yaml, "OPERATOR_INITIAL_DESIRED_RELEASE", "none");
    append_env_value(&mut yaml, "OPERATOR_SETUP_METHOD", "manual");
    append_env_value(&mut yaml, "DATA_DIR", "/var/lib/operator");
    if options.log_collector.is_some() {
        append_env_value(&mut yaml, "OTLP_HOST", "0.0.0.0");
        append_env_value(&mut yaml, "OTLP_PORT", "8080");
        append_env_value(
            &mut yaml,
            "COLLECTOR_TOKEN_FILE",
            "/etc/operator/secrets/collector-token",
        );
    }
    append_env_value(
        &mut yaml,
        "SYNC_TOKEN_FILE",
        "/etc/operator/secrets/sync-token",
    );
    append_env_value(
        &mut yaml,
        "OPERATOR_ENCRYPTION_KEY_FILE",
        "/etc/operator/secrets/encryption-key",
    );
    append_env_value(&mut yaml, "SYNC_INTERVAL", "30");
    if options.log_collector.is_some() {
        yaml.push_str("          ports:\n");
        yaml.push_str("            - name: http\n");
        yaml.push_str("              containerPort: 8080\n");
    }
    yaml.push_str("          volumeMounts:\n");
    yaml.push_str("            - name: credentials\n");
    yaml.push_str("              mountPath: /etc/operator/secrets\n");
    yaml.push_str("              readOnly: true\n");
    yaml.push_str("            - name: identity\n");
    yaml.push_str("              mountPath: /var/lib/operator\n");
    yaml.push_str("            - name: tmp\n");
    yaml.push_str("              mountPath: /tmp\n");
    yaml.push_str("          resources:\n");
    yaml.push_str("            requests:\n");
    yaml.push_str("              cpu: 50m\n");
    yaml.push_str("              memory: 128Mi\n");
    yaml.push_str("            limits:\n");
    yaml.push_str("              cpu: 500m\n");
    yaml.push_str("              memory: 512Mi\n");
    yaml.push_str("      volumes:\n");
    yaml.push_str("        - name: credentials\n");
    yaml.push_str("          secret:\n");
    yaml.push_str(&format!(
        "            secretName: {}\n",
        yaml_string(operator_name)
    ));
    yaml.push_str("            defaultMode: 384\n");
    yaml.push_str("        - name: identity\n");
    yaml.push_str("          persistentVolumeClaim:\n");
    yaml.push_str(&format!(
        "            claimName: {}\n",
        yaml_string(identity_pvc_name)
    ));
    yaml.push_str("        - name: tmp\n");
    yaml.push_str("          emptyDir:\n");
    yaml.push_str("            sizeLimit: 64Mi\n");
    yaml
}

fn operator_service_doc(
    namespace: &str,
    operator_name: &str,
    labels: &BTreeMap<String, String>,
) -> String {
    let mut yaml = operator_metadata_doc("v1", "Service", namespace, operator_name, labels);
    yaml.push_str("spec:\n");
    yaml.push_str("  type: ClusterIP\n");
    yaml.push_str("  selector:\n");
    append_operator_selector_labels(&mut yaml, labels, 4);
    yaml.push_str("  ports:\n");
    yaml.push_str("    - name: http\n");
    yaml.push_str("      port: 8080\n");
    yaml.push_str("      targetPort: http\n");
    yaml
}

fn operator_log_collector_service_account_doc(
    namespace: &str,
    operator_name: &str,
    labels: &BTreeMap<String, String>,
) -> String {
    let name = format!("{operator_name}-whitelabeled-log-collector");
    let mut yaml = operator_metadata_doc("v1", "ServiceAccount", namespace, &name, labels);
    yaml.push_str("automountServiceAccountToken: true\n");
    yaml
}

fn operator_log_collector_role_doc(
    namespace: &str,
    operator_name: &str,
    labels: &BTreeMap<String, String>,
) -> String {
    let name = format!("{operator_name}-whitelabeled-log-collector");
    let mut yaml = operator_metadata_doc(
        "rbac.authorization.k8s.io/v1",
        "Role",
        namespace,
        &name,
        labels,
    );
    yaml.push_str("rules:\n");
    yaml.push_str("  - apiGroups: [\"\"]\n");
    yaml.push_str("    resources: [\"pods\"]\n");
    yaml.push_str("    verbs: [\"get\", \"list\", \"watch\"]\n");
    yaml
}

fn operator_log_collector_role_binding_doc(
    namespace: &str,
    operator_name: &str,
    labels: &BTreeMap<String, String>,
) -> String {
    let name = format!("{operator_name}-whitelabeled-log-collector");
    let mut yaml = operator_metadata_doc(
        "rbac.authorization.k8s.io/v1",
        "RoleBinding",
        namespace,
        &name,
        labels,
    );
    yaml.push_str("roleRef:\n");
    yaml.push_str("  apiGroup: rbac.authorization.k8s.io\n");
    yaml.push_str("  kind: Role\n");
    yaml.push_str(&format!("  name: {}\n", yaml_string(&name)));
    yaml.push_str("subjects:\n");
    yaml.push_str("  - kind: ServiceAccount\n");
    yaml.push_str(&format!("    name: {}\n", yaml_string(&name)));
    yaml.push_str(&format!("    namespace: {}\n", yaml_string(namespace)));
    yaml
}

fn operator_log_collector_configmap_doc(
    namespace: &str,
    operator_name: &str,
    observed_namespace: &str,
    labels: &BTreeMap<String, String>,
) -> String {
    let name = format!("{operator_name}-whitelabeled-log-collector");
    let mut yaml = operator_metadata_doc("v1", "ConfigMap", namespace, &name, labels);
    yaml.push_str("data:\n");
    yaml.push_str("  collector.conf: |\n");
    yaml.push_str("    [SERVICE]\n");
    yaml.push_str("        Flush        2\n");
    yaml.push_str("        Log_Level    info\n");
    yaml.push_str("        Parsers_File parsers.conf\n");
    yaml.push_str("        storage.path /buffers\n");
    yaml.push_str("        storage.sync normal\n");
    yaml.push_str("        storage.backlog.mem_limit 64M\n\n");
    yaml.push_str("    [INPUT]\n");
    yaml.push_str("        Name              tail\n");
    yaml.push_str(&format!(
        "        Path              /var/log/pods/{}_*/*/*.log\n",
        observed_namespace
    ));
    yaml.push_str(&format!(
        "        Exclude_Path      /var/log/pods/{}_{}-*/*/*.log\n",
        observed_namespace, operator_name
    ));
    yaml.push_str("        Path_Key          filename\n");
    // Built-in multiline parsers auto-detect the runtime log format: `cri` for
    // containerd (EKS/GKE/AKS, k8s >=1.24) and `docker` for the docker-json format
    // used by Docker-runtime clusters (Docker Desktop, OrbStack, legacy on-prem).
    yaml.push_str("        multiline.parser  docker, cri\n");
    yaml.push_str("        Tag               kube.*\n");
    yaml.push_str(&format!(
        "        DB                /buffers/{operator_name}-whitelabeled-log-collector.db\n"
    ));
    yaml.push_str("        Mem_Buf_Limit     64MB\n");
    yaml.push_str("        Skip_Long_Lines   On\n");
    yaml.push_str("        Read_from_Head    On\n");
    yaml.push_str("        Refresh_Interval  5\n");
    yaml.push_str("        storage.type      filesystem\n\n");
    yaml.push_str("    [FILTER]\n");
    yaml.push_str("        Name                kubernetes\n");
    yaml.push_str("        Match               kube.*\n");
    yaml.push_str("        Merge_Log           Off\n");
    yaml.push_str("        Keep_Log            On\n");
    yaml.push_str("        Labels              On\n");
    yaml.push_str("        Annotations         Off\n\n");
    yaml.push_str("    [OUTPUT]\n");
    yaml.push_str("        Name          http\n");
    // Without a Match the router never routes the tailed kube.* records to this
    // output ("NO match for http.0 output instance"), so no pod logs are shipped.
    yaml.push_str("        Match         kube.*\n");
    yaml.push_str(&format!(
        "        Host          {operator_name}.{namespace}.svc.cluster.local\n"
    ));
    yaml.push_str("        Port          8080\n");
    yaml.push_str("        URI           /internal/logs\n");
    yaml.push_str("        Format        json\n");
    yaml.push_str("        Json_Date_Key observed_at\n");
    yaml.push_str("        Header        Authorization Bearer ${COLLECTOR_TOKEN}\n\n");
    yaml.push_str("  parsers.conf: |\n");
    yaml.push_str("    [PARSER]\n");
    yaml.push_str("        Name        cri\n");
    yaml.push_str("        Format      regex\n");
    yaml.push_str(
        "        Regex       ^(?<time>[^ ]+) (?<stream>stdout|stderr) (?<logtag>[^ ]*) (?<log>.*)$\n",
    );
    yaml.push_str("        Time_Key    time\n");
    yaml.push_str("        Time_Format %Y-%m-%dT%H:%M:%S.%L%z\n");
    yaml
}

fn operator_log_collector_daemonset_doc(
    namespace: &str,
    operator_name: &str,
    image: &str,
    labels: &BTreeMap<String, String>,
) -> String {
    let name = format!("{operator_name}-whitelabeled-log-collector");
    let mut yaml = operator_metadata_doc("apps/v1", "DaemonSet", namespace, &name, labels);
    yaml.push_str("spec:\n");
    yaml.push_str("  selector:\n");
    yaml.push_str("    matchLabels:\n");
    append_operator_selector_labels(&mut yaml, labels, 6);
    yaml.push_str("  template:\n");
    yaml.push_str("    metadata:\n");
    yaml.push_str("      labels:\n");
    append_operator_labels(&mut yaml, labels, 8);
    yaml.push_str("    spec:\n");
    yaml.push_str(&format!(
        "      serviceAccountName: {}\n",
        yaml_string(&name)
    ));
    yaml.push_str("      tolerations:\n");
    yaml.push_str("        - operator: Exists\n");
    yaml.push_str("      containers:\n");
    yaml.push_str("        - name: collector\n");
    yaml.push_str(&format!("          image: {}\n", yaml_string(image)));
    yaml.push_str("          imagePullPolicy: IfNotPresent\n");
    yaml.push_str("          args: [\"-c\", \"/collector/etc/collector.conf\"]\n");
    yaml.push_str("          env:\n");
    yaml.push_str("            - name: COLLECTOR_TOKEN\n");
    yaml.push_str("              valueFrom:\n");
    yaml.push_str("                secretKeyRef:\n");
    yaml.push_str(&format!(
        "                  name: {}\n",
        yaml_string(operator_name)
    ));
    yaml.push_str("                  key: collector-token\n");
    yaml.push_str("          volumeMounts:\n");
    yaml.push_str("            - name: config\n");
    yaml.push_str("              mountPath: /collector/etc\n");
    yaml.push_str("              readOnly: true\n");
    yaml.push_str("            - name: varlog\n");
    yaml.push_str("              mountPath: /var/log\n");
    yaml.push_str("              readOnly: true\n");
    // Docker-runtime clusters symlink /var/log/pods/.../*.log to the json log files
    // under /var/lib/docker/containers, so the symlink targets must be mounted too or
    // fluent-bit reads nothing. Harmless on containerd nodes (DirectoryOrCreate).
    yaml.push_str("            - name: dockercontainers\n");
    yaml.push_str("              mountPath: /var/lib/docker/containers\n");
    yaml.push_str("              readOnly: true\n");
    yaml.push_str("            - name: buffers\n");
    yaml.push_str("              mountPath: /buffers\n");
    yaml.push_str("      volumes:\n");
    yaml.push_str("        - name: config\n");
    yaml.push_str("          configMap:\n");
    yaml.push_str(&format!("            name: {}\n", yaml_string(&name)));
    yaml.push_str("        - name: varlog\n");
    yaml.push_str("          hostPath:\n");
    yaml.push_str("            path: /var/log\n");
    yaml.push_str("            type: Directory\n");
    yaml.push_str("        - name: dockercontainers\n");
    yaml.push_str("          hostPath:\n");
    yaml.push_str("            path: /var/lib/docker/containers\n");
    yaml.push_str("            type: DirectoryOrCreate\n");
    yaml.push_str("        - name: buffers\n");
    yaml.push_str("          emptyDir: {}\n");
    yaml
}

fn operator_metadata_doc(
    api_version: &str,
    kind: &str,
    namespace: &str,
    name: &str,
    labels: &BTreeMap<String, String>,
) -> String {
    let mut yaml = String::new();
    yaml.push_str(&format!("apiVersion: {}\n", yaml_string(api_version)));
    yaml.push_str(&format!("kind: {}\n", yaml_string(kind)));
    yaml.push_str("metadata:\n");
    yaml.push_str(&format!("  name: {}\n", yaml_string(name)));
    yaml.push_str(&format!("  namespace: {}\n", yaml_string(namespace)));
    yaml.push_str("  labels:\n");
    append_operator_labels(&mut yaml, labels, 4);
    yaml
}

fn append_operator_selector_labels(
    yaml: &mut String,
    labels: &BTreeMap<String, String>,
    indent: usize,
) {
    for key in ["app.kubernetes.io/name", "app.kubernetes.io/instance"] {
        if let Some(value) = labels.get(key) {
            yaml.push_str(&format!(
                "{}{}: {}\n",
                " ".repeat(indent),
                yaml_key(key),
                yaml_string(value)
            ));
        }
    }
}

fn append_operator_labels(yaml: &mut String, labels: &BTreeMap<String, String>, indent: usize) {
    for (key, value) in labels {
        yaml.push_str(&format!(
            "{}{}: {}\n",
            " ".repeat(indent),
            yaml_key(key),
            yaml_string(value)
        ));
    }
}

fn append_env_value(yaml: &mut String, name: &str, value: &str) {
    yaml.push_str(&format!("            - name: {}\n", yaml_string(name)));
    yaml.push_str(&format!("              value: {}\n", yaml_string(value)));
}
