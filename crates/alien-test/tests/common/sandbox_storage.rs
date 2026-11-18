//! Checks for the `sandbox-storage` app: a sandbox whose `/workspace` is persistent storage. The
//! test drives the binding the deployment's controller published, as the deployment's own code
//! would (a persistent storage sandbox is never published remotely): it writes a tree, syncs,
//! terminates, restores the key into a new sandbox, reads a file without a sandbox, has a stale
//! writer refused and erases the key. It calls with the test's target credentials rather than a
//! workload identity: it first gives its own principal exactly the role assignments setup made
//! for the app's execution identity (sandbox group and store scopes), so those grants are what
//! it runs on.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use alien_bindings::traits::{
    CommandOutput, CreateSandboxRequest, RunCommandRequest, Sandbox, SandboxStorageState,
};
use alien_core::Platform;
use anyhow::{bail, ensure, Context};
use futures::StreamExt;

const PATH: &str = "/workspace";

/// The tree the checks write: modes, an executable, nested directories, a hard link, a relative
/// and an absolute symlink (one pointing out of the tree, which must come back as a link and not
/// as the target's bytes), a SQLite database in WAL mode, a git repository, and an excluded
/// `tmp/` that must not come back.
const SEED: &str = r#"
set -eu
cd /workspace
mkdir -p src/deep/er tmp
printf 'hello from the first sandbox\n' > notes.txt
printf '#!/bin/sh\necho run\n' > src/run.sh && chmod 0755 src/run.sh
printf 'secret' > src/private && chmod 0600 src/private
head -c 1048576 /dev/urandom > src/deep/er/blob.bin
ln src/deep/er/blob.bin src/blob-hardlink.bin
ln -s deep/er/blob.bin src/blob-symlink
ln -s /etc/passwd outside-link
touch -d '2020-01-02 03:04:05' notes.txt
printf 'not replicated\n' > tmp/scratch
sqlite3 db.sqlite 'PRAGMA journal_mode=WAL; CREATE TABLE t(i INTEGER PRIMARY KEY, v TEXT);'
sqlite3 db.sqlite "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM c WHERE x<5000) INSERT INTO t(v) SELECT hex(randomblob(32)) FROM c;"
git init -q repo && cd repo && git config user.email t@example.com && git config user.name t \
  && printf 'a\n' > a && git add a && git commit -qm one && printf 'b\n' > b && git add b && git commit -qm two
"#;

/// One line per entry: path, type, mode, owner, size, mtime, content hash or link target, and a
/// hard-link group (the first path sharing its inode), sorted. `tmp/` is excluded as declared.
const MANIFEST: &str = r#"
import hashlib, os, stat, sys
root = '/workspace'
first = {}
out = []
for d, dirs, files in os.walk(root):
    dirs.sort()
    rel_d = os.path.relpath(d, root)
    if rel_d == 'tmp' or rel_d.startswith('tmp/'):
        dirs[:] = []
        continue
    for name in sorted(dirs + files):
        p = os.path.join(d, name)
        rel = os.path.relpath(p, root)
        if rel == 'tmp':
            continue
        st = os.lstat(p)
        kind = 'l' if stat.S_ISLNK(st.st_mode) else 'd' if stat.S_ISDIR(st.st_mode) else 'f'
        extra = ''
        if kind == 'l':
            extra = os.readlink(p)
        elif kind == 'f':
            h = hashlib.sha256()
            with open(p, 'rb') as f:
                for chunk in iter(lambda: f.read(1 << 20), b''):
                    h.update(chunk)
            extra = h.hexdigest()
        link = ''
        if kind == 'f' and st.st_nlink > 1:
            link = first.setdefault((st.st_dev, st.st_ino), rel)
        mtime = int(st.st_mtime) if kind != 'd' else 0
        out.append(f'{rel}\t{kind}\t{stat.S_IMODE(st.st_mode):o}\t{st.st_uid}:{st.st_gid}\t{st.st_size if kind == "f" else 0}\t{mtime}\t{extra}\t{link}')
print('\n'.join(sorted(out)))
"#;

struct Output {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

async fn sh(sandbox: &Arc<dyn Sandbox>, id: &str, script: &str) -> anyhow::Result<Output> {
    let mut stream = sandbox
        .run_command(
            id,
            RunCommandRequest {
                command: "sh".into(),
                args: vec!["-c".into(), script.into()],
                cwd: None,
                env: Default::default(),
                timeout: Duration::from_secs(600),
            },
        )
        .await
        .context("run a command")?;
    let mut out = Output {
        code: None,
        stdout: String::new(),
        stderr: String::new(),
    };
    while let Some(item) = stream.next().await {
        match item.context("command output")? {
            CommandOutput::Stdout { data, .. } => {
                out.stdout.push_str(&String::from_utf8_lossy(&data))
            }
            CommandOutput::Stderr { data, .. } => {
                out.stderr.push_str(&String::from_utf8_lossy(&data))
            }
            CommandOutput::Exit { code, .. } => out.code = Some(code),
        }
    }
    Ok(out)
}

async fn ok(sandbox: &Arc<dyn Sandbox>, id: &str, script: &str) -> anyhow::Result<String> {
    let out = sh(sandbox, id, script).await?;
    ensure!(
        out.code == Some(0),
        "`{}` exited {:?}: {}{}",
        script.trim().lines().next().unwrap_or_default(),
        out.code,
        out.stdout,
        out.stderr
    );
    Ok(out.stdout)
}

async fn manifest(sandbox: &Arc<dyn Sandbox>, id: &str) -> anyhow::Result<String> {
    let script = format!("python3 - <<'PY'\n{MANIFEST}\nPY");
    ok(sandbox, id, &script).await
}

async fn create(sandbox: &Arc<dyn Sandbox>, key: &str) -> anyhow::Result<String> {
    let started = std::time::Instant::now();
    // Role assignments made a moment ago take a few minutes to reach the data planes.
    let created = loop {
        match sandbox
            .create(CreateSandboxRequest {
                storage_key: Some(key.to_string()),
                ..Default::default()
            })
            .await
        {
            Ok(created) => break created,
            Err(error)
                if started.elapsed() < Duration::from_secs(600)
                    && format!("{error:?}").contains("403") =>
            {
                eprintln!("create refused (403), waiting for role propagation: {error}");
                tokio::time::sleep(Duration::from_secs(20)).await;
            }
            Err(error) => {
                return Err(anyhow::anyhow!("{error}"))
                    .with_context(|| format!("create a sandbox for key {key}"))
            }
        }
    };
    eprintln!(
        "sandbox {} for key {key} ready in {:.1}s",
        created.sandbox_id,
        started.elapsed().as_secs_f64()
    );
    Ok(created.sandbox_id)
}

async fn wait_for_state(
    sandbox: &Arc<dyn Sandbox>,
    id: &str,
    want: SandboxStorageState,
) -> anyhow::Result<()> {
    let started = std::time::Instant::now();
    loop {
        let status = sandbox
            .storage_status(id)
            .await?
            .context("the sandbox reports no persistent storage")?;
        if status.state == want {
            return Ok(());
        }
        if started.elapsed() > Duration::from_secs(120) {
            bail!("sandbox {id} stayed {status:?}, expected {want:?}");
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

/// The binding the controller of sandbox `agents` publishes, from the deployment's stack state.
async fn published_binding(
    deployment: &alien_test::TestDeployment,
) -> anyhow::Result<serde_json::Value> {
    use alien_infra::ResourceController as _;
    let response = deployment
        .manager()
        .client()
        .get_deployment()
        .id(&deployment.id)
        .send()
        .await
        .map_err(|error| anyhow::anyhow!("get_deployment failed: {error}"))?;
    let state: alien_core::StackState = serde_json::from_value(
        response
            .into_inner()
            .stack_state
            .context("deployment is missing stack_state")?,
    )
    .context("failed to parse stack_state")?;
    let internal = state
        .resources
        .get("agents")
        .and_then(|resource| resource.internal_state.clone())
        .context("sandbox 'agents' has no controller state")?;
    let controller = alien_infra::deserialize_controller(internal).context("controller state")?;
    controller
        .get_binding_params()
        .map_err(|error| anyhow::anyhow!("binding params: {error}"))?
        .context("the controller publishes no binding yet")
}

/// The whole persistent storage path through the deployment's own binding.
pub async fn check_persistent_storage(
    deployment: &alien_test::TestDeployment,
    platform: Platform,
) -> anyhow::Result<()> {
    use alien_bindings::BindingsProviderApi as _;
    ensure!(
        platform == Platform::Azure,
        "only the Azure path is wired here"
    );
    let binding = published_binding(deployment).await?;
    eprintln!("published binding: {binding}");
    ensure!(
        binding.get("persistentStorage").is_some(),
        "the published binding carries no persistentStorage: {binding}"
    );
    let target = alien_test::TestConfig::from_env()
        .azure_target
        .context("Azure target credentials")?;
    let client_config = alien_core::ClientConfig::Azure(Box::new(alien_core::AzureClientConfig {
        subscription_id: target.subscription_id,
        tenant_id: target.tenant_id,
        region: Some(target.region),
        credentials: alien_core::AzureCredentials::ServicePrincipal {
            client_id: target.client_id,
            client_secret: target.client_secret,
        },
        service_overrides: None,
    }));
    let provider = alien_bindings::BindingsProvider::new(
        client_config.clone(),
        std::collections::HashMap::from([("agents".to_string(), binding.clone())]),
    )
    .map_err(|error| anyhow::anyhow!("bindings provider: {error}"))?;
    let sandbox: Arc<dyn Sandbox> = provider
        .load_sandbox("agents")
        .await
        .map_err(|error| anyhow::anyhow!("load the sandbox binding: {error}"))?;
    let capabilities = sandbox.capabilities();
    ensure!(
        capabilities.persistent_storage,
        "the capability table does not report persistentStorage: {capabilities:?}"
    );

    let field = |name: &str| {
        binding
            .get(name)
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .with_context(|| format!("the binding's {name}"))
    };
    let group = field("sandboxGroup")?;
    let plane = alien_azure_clients::azure::sandbox_data_plane::AzureSandboxDataPlaneClient::new(
        reqwest::Client::new(),
        &field("region")?,
        &field("resourceGroup")?,
        alien_azure_clients::AzureTokenCache::new(match &client_config {
            alien_core::ClientConfig::Azure(config) => (**config).clone(),
            _ => unreachable!("an Azure config"),
        }),
    );
    let store_host = format!(
        "{}.blob.core.windows.net",
        binding
            .pointer("/persistentStorage/store/account")
            .and_then(|v| v.as_str())
            .context("the binding's store account")?
    );
    let container = binding
        .pointer("/persistentStorage/store/container")
        .and_then(|v| v.as_str())
        .context("the binding's store container")?
        .to_string();
    let internet = field("egress")? == "internet";
    let abrupt = Abrupt {
        plane,
        group,
        store_host,
        container,
        internet,
    };

    let token = alien_azure_clients::AzureTokenCache::new(match &client_config {
        alien_core::ClientConfig::Azure(config) => (**config).clone(),
        _ => unreachable!("an Azure config"),
    });
    let principal = alien_test::TestConfig::from_env()
        .azure_target
        .and_then(|t| t.principal_id)
        .context("AZURE_TARGET_PRINCIPAL_ID")?;
    let mirrored = mirror_execution_roles(&token, &binding, &principal).await?;
    let result = all_checks(&sandbox, &abrupt).await;
    for id in mirrored {
        if let Err(error) = arm(&token, reqwest::Method::DELETE, &id, None).await {
            eprintln!("cleanup: role assignment {id}: {error}");
        }
    }
    result
}

/// ARM call with the test's credentials; the answer's JSON, or the error with its body.
async fn arm(
    token: &alien_azure_clients::AzureTokenCache,
    method: reqwest::Method,
    path: &str,
    body: Option<serde_json::Value>,
) -> anyhow::Result<serde_json::Value> {
    let bearer = token
        .get_bearer_token_with_scope("https://management.azure.com/.default")
        .await
        .map_err(|e| anyhow::anyhow!("ARM token: {e}"))?;
    let separator = if path.contains('?') { '&' } else { '?' };
    let url = format!(
        "https://management.azure.com{path}{separator}api-version=2022-04-01"
    );
    let mut request = reqwest::Client::new()
        .request(method.clone(), &url)
        .bearer_auth(bearer);
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await.context("ARM request")?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    ensure!(status.is_success(), "{method} {path}: {status}: {text}");
    Ok(serde_json::from_str(&text).unwrap_or(serde_json::Value::Null))
}

/// Gives `principal` every role assignment setup made in the deployment's resource group for
/// another service principal at the sandbox group or the store (the execution identity's
/// `sandbox/execute` and store grants), at the same scopes. Returns the new assignments' ids.
async fn mirror_execution_roles(
    token: &alien_azure_clients::AzureTokenCache,
    binding: &serde_json::Value,
    principal: &str,
) -> anyhow::Result<Vec<String>> {
    let config = token.config();
    let group = binding["sandboxGroup"].as_str().context("sandboxGroup")?;
    let account = binding["persistentStorage"]["store"]["account"]
        .as_str()
        .context("store account")?;
    let rg = format!(
        "/subscriptions/{}/resourceGroups/{}",
        config.subscription_id,
        binding["resourceGroup"].as_str().context("resourceGroup")?
    );
    let listed = arm(
        token,
        reqwest::Method::GET,
        &format!("{rg}/providers/Microsoft.Authorization/roleAssignments"),
        None,
    )
    .await?;
    let mut created = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for assignment in listed["value"].as_array().cloned().unwrap_or_default() {
        let properties = &assignment["properties"];
        let scope = properties["scope"].as_str().unwrap_or_default().to_string();
        let role = properties["roleDefinitionId"].as_str().unwrap_or_default().to_string();
        let lower = scope.to_ascii_lowercase();
        let ours = lower.contains(&format!("/sandboxgroups/{}", group.to_ascii_lowercase()))
            || lower.contains(&format!("/storageaccounts/{}", account.to_ascii_lowercase()));
        if !ours
            || properties["principalId"].as_str() == Some(principal)
            || properties["principalType"].as_str() != Some("ServicePrincipal")
            || !seen.insert((scope.clone(), role.clone()))
        {
            continue;
        }
        let id = format!(
            "{scope}/providers/Microsoft.Authorization/roleAssignments/{}",
            uuid::Uuid::new_v4()
        );
        arm(
            token,
            reqwest::Method::PUT,
            &id,
            Some(serde_json::json!({
                "properties": {
                    "roleDefinitionId": role,
                    "principalId": principal,
                    "principalType": "ServicePrincipal",
                }
            })),
        )
        .await?;
        eprintln!("mirrored {role} at {scope}");
        created.push(id);
    }
    ensure!(
        !created.is_empty(),
        "setup made no role assignment at the sandbox group or the store"
    );
    Ok(created)
}

async fn all_checks(sandbox: &Arc<dyn Sandbox>, abrupt: &Abrupt) -> anyhow::Result<()> {

    let stamp = chrono::Utc::now().timestamp_millis();
    let mut result = Ok(());
    for (key, check) in [
        (format!("e2e-{stamp}"), Check::Main),
        (format!("e2e-max-{stamp}"), Check::MaxSize),
        (format!("e2e-kill-{stamp}"), Check::Kills),
    ] {
        let mut live: Vec<String> = Vec::new();
        let outcome = match check {
            Check::Main => {
                run_checks(
                    sandbox,
                    &abrupt.store_host,
                    &abrupt.container,
                    abrupt.internet,
                    &key,
                    &mut live,
                )
                .await
            },
            Check::MaxSize => max_size(sandbox, &key, &mut live).await,
            Check::Kills => kills(sandbox, abrupt, &key, *KILLS, &mut live).await,
        };
        for id in live {
            if let Err(error) = sandbox.terminate(&id).await {
                eprintln!("cleanup: terminate {id}: {error}");
            }
        }
        if let Err(error) = sandbox.delete_storage(&key).await {
            eprintln!("cleanup: delete key {key}: {error}");
        }
        if let Err(error) = outcome {
            eprintln!("check {check:?} failed: {error:#}");
            if result.is_ok() {
                result = Err(error.context(format!("{check:?}")));
            }
        }
    }
    result
}

#[derive(Debug, Clone, Copy)]
enum Check {
    Main,
    MaxSize,
    Kills,
}

/// Abrupt terminations: how many, from the environment (`ALIEN_TEST_STORAGE_KILLS`), 20 by default.
static KILLS: std::sync::LazyLock<usize> = std::sync::LazyLock::new(|| {
    std::env::var("ALIEN_TEST_STORAGE_KILLS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20)
});

/// Deletes a sandbox through the data plane alone: no sync, no word to its agent.
struct Abrupt {
    plane: alien_azure_clients::azure::sandbox_data_plane::AzureSandboxDataPlaneClient,
    group: String,
    /// The store account's blob host, which the platform opens for the agent only.
    store_host: String,
    container: String,
    /// The sandbox's egress is `internet` (through the gateway), not `none`.
    internet: bool,
}

impl Abrupt {
    async fn kill(&self, id: &str) -> anyhow::Result<()> {
        use alien_azure_clients::azure::sandbox_data_plane::SandboxDataPlaneApi as _;
        self.plane
            .delete_sandbox(&self.group, id)
            .await
            .map_err(|error| anyhow::anyhow!("delete sandbox {id}: {error}"))
    }
}

/// A commit above the declared `maxSize` (2Gi here) is refused whole: the head stays where it
/// was, and the next commit under the ceiling goes through. The file is sparse, so the walk's
/// size refuses it before anything is read.
async fn max_size(
    sandbox: &Arc<dyn Sandbox>,
    key: &str,
    live: &mut Vec<String>,
) -> anyhow::Result<()> {
    let id = create(sandbox, key).await?;
    live.push(id.clone());
    ok(sandbox, &id, "echo small > /workspace/small.txt").await?;
    let before = sandbox.sync_storage(&id).await.context("first sync")?;
    ok(sandbox, &id, "truncate -s 2200M /workspace/big.sparse").await?;
    let refused = sandbox.sync_storage(&id).await;
    eprintln!("sync above maxSize: {refused:?}");
    ensure!(refused.is_err(), "a commit above maxSize went through: {refused:?}");
    let status = sandbox.storage_status(&id).await?.context("status")?;
    ensure!(
        status.head == before.head,
        "the head moved past {:?} to {:?}",
        before.head,
        status.head
    );
    ok(sandbox, &id, "rm /workspace/big.sparse && echo again > /workspace/small.txt").await?;
    let after = sandbox.sync_storage(&id).await.context("sync under maxSize")?;
    ensure!(after.head > before.head, "no commit after the ceiling cleared: {after:?}");
    eprintln!("maxSize: head stayed {:?} while above, then {:?}", before.head, after.head);
    Ok(())
}

/// The writer the kills interrupt: SQLite in WAL with `synchronous=full` every 50 ms, a file
/// replaced by rename each time, and an empty git commit each second. It stops by itself.
const WRITER: &str = r#"
import os, sqlite3, subprocess, time
root = '/workspace/kill'
db = sqlite3.connect(root + '/w.db', isolation_level=None)
db.execute('PRAGMA journal_mode=WAL')
db.execute('PRAGMA synchronous=FULL')
db.execute('CREATE TABLE IF NOT EXISTS t(ts REAL)')
last_git = 0.0
end = time.time() + 60
while time.time() < end:
    now = time.time()
    db.execute('INSERT INTO t(ts) VALUES (?)', (now,))
    with open(root + '/latest.tmp', 'w') as f:
        f.write(repr(now))
        f.flush()
        os.fsync(f.fileno())
    os.rename(root + '/latest.tmp', root + '/latest')
    if now - last_git > 1:
        subprocess.run(['git', '-C', root + '/repo', 'commit', '--allow-empty', '-qm', repr(now)], check=True)
        last_git = now
    time.sleep(0.05)
"#;

/// Abrupt terminations under the writer: each round restores the key into a new sandbox, checks
/// SQLite and git, measures what the previous kill lost (the kill's guest time minus the newest
/// restored row), runs the writer for a while and deletes the sandbox through the data plane.
async fn kills(
    sandbox: &Arc<dyn Sandbox>,
    abrupt: &Abrupt,
    key: &str,
    rounds: usize,
    live: &mut Vec<String>,
) -> anyhow::Result<()> {
    let mut lost: Vec<f64> = Vec::new();
    let mut killed_at: Option<f64> = None;
    for round in 0..=rounds {
        let id = create(sandbox, key).await?;
        live.push(id.clone());
        if round == 0 {
            ok(
                sandbox,
                &id,
                "mkdir -p /workspace/kill && cd /workspace/kill && git init -q repo && git -C repo config user.email t@example.com && git -C repo config user.name t && git -C repo commit --allow-empty -qm root",
            )
            .await?;
        } else {
            let checked = ok(
                sandbox,
                &id,
                "cd /workspace/kill && sqlite3 w.db 'PRAGMA integrity_check' && sqlite3 w.db 'SELECT max(ts) FROM t' && git -C repo fsck --full >/dev/null 2>&1 && echo fsck-ok && cat latest && echo",
            )
            .await
            .with_context(|| format!("restore after kill {round}"))?;
            let lines: Vec<&str> = checked.lines().collect();
            ensure!(
                lines.first() == Some(&"ok") && lines.get(2) == Some(&"fsck-ok"),
                "restore after kill {round}: {checked}"
            );
            let newest: f64 = lines
                .get(1)
                .and_then(|l| l.trim().parse().ok())
                .with_context(|| format!("newest row after kill {round}: {checked}"))?;
            let seconds = killed_at.expect("a kill before") - newest;
            eprintln!("kill {round}: lost {seconds:.2}s");
            lost.push(seconds);
        }
        if round == rounds {
            break;
        }
        let writer = {
            let sandbox = Arc::clone(sandbox);
            let id = id.clone();
            let script = format!("python3 - <<'PY'
{WRITER}
PY");
            tokio::spawn(async move { sh(&sandbox, &id, &script).await })
        };
        tokio::time::sleep(Duration::from_secs(8 + (round as u64 % 5))).await;
        let now = ok(sandbox, &id, "date +%s.%N").await?;
        abrupt.kill(&id).await?;
        killed_at = Some(now.trim().parse().context("guest time")?);
        live.retain(|l| l != &id);
        writer.abort();
        // The data plane deletes asynchronously; the next sandbox's claim supersedes this one
        // whether or not it is gone yet.
    }
    let mut sorted = lost.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let at = |q: f64| sorted[((sorted.len() as f64 * q).ceil() as usize).saturating_sub(1)];
    eprintln!(
        "kills: {} rounds, seconds lost p50 {:.2} p95 {:.2} max {:.2}: {:?}",
        sorted.len(),
        at(0.5),
        at(0.95),
        sorted.last().copied().unwrap_or_default(),
        lost
    );
    ensure!(at(0.95) <= 3.0, "p95 seconds lost {:.2} above 3", at(0.95));
    Ok(())
}

async fn run_checks(
    sandbox: &Arc<dyn Sandbox>,
    store_host: &str,
    container: &str,
    internet: bool,
    key: &str,
    live: &mut Vec<String>,
) -> anyhow::Result<()> {
    // 1. Write the tree and sync it.
    let first = create(sandbox, key).await?;
    live.push(first.clone());
    let status = sandbox.storage_status(&first).await?.context("no status")?;
    ensure!(
        status.key.as_deref() == Some(key),
        "status names {status:?}"
    );
    ok(sandbox, &first, SEED).await.context("seed the tree")?;
    let written = manifest(sandbox, &first).await?;
    let synced = sandbox.sync_storage(&first).await.context("sync")?;
    ensure!(
        synced.state == SandboxStorageState::Replicating && synced.head.is_some(),
        "sync answered {synced:?}"
    );
    eprintln!("synced head {:?}", synced.head);

    // 2. The command's uid sees no rights and opens no direct connection, to the store or
    //    anywhere: by name, and by the address the test host resolves the store to (no DNS
    //    needed in the guest). Under `internet` the command's proxy (the gateway) still reaches
    //    the internet, and the store answers it without access, as it would anyone without a SAS.
    let store_ip = tokio::net::lookup_host((store_host, 443))
        .await
        .context("resolve the store host")?
        .find(|a| a.is_ipv4())
        .context("the store host has no IPv4 address")?
        .ip();
    let direct = "curl --noproxy '*' -sS -m 8 -o /dev/null";
    let mut script = format!(
        "echo uid=$(id -u); \
{direct} https://{store_host}/ && echo REACHED_STORE || echo store-refused; \
{direct} --resolve {store_host}:443:{store_ip} https://{store_host}/ && echo REACHED_STORE || echo store-ip-refused; \
{direct} https://www.microsoft.com/ && echo REACHED_DIRECT || echo direct-refused; "
    );
    if internet {
        script += &format!(
            "curl -sS -m 20 -o /dev/null https://www.microsoft.com/ && echo gateway-ok || echo GATEWAY_FAILED; \
echo store-via-gateway=$(curl -s -m 20 -o /dev/null -w '%{{http_code}}' https://{store_host}/{container}/{key}/HEAD); "
        );
    } else {
        script += "curl -sS -m 8 -o /dev/null https://management.azure.com/ && echo REACHED_OUT || echo outbound-refused; ";
    }
    // `si[g]=` matches a SAS but not this script's own command line, nor grep's arguments; each
    // hit names its process so a real one can be told apart.
    script += r#"env | grep -c 'si[g]=' || true; cat /proc/1/environ >/dev/null 2>&1 && echo READ_PID1_ENV || echo pid1-env-denied; echo begin-grep;
for f in $(grep -rls 'si[g]=' /proc/[0-9]*/environ /proc/[0-9]*/cmdline /tmp /workspace /opt/alien 2>/dev/null | head -5); do echo "$f $(cat "${f%/*}/comm" 2>/dev/null)"; done; echo end-grep"#;
    let probe = sh(sandbox, &first, &script).await?;
    eprintln!("command-uid probe:\n{}{}", probe.stdout, probe.stderr);
    let says = |line: &str| probe.stdout.lines().any(|l| l == line);
    ensure!(
        !probe.stdout.contains("REACHED_STORE") && says("store-refused") && says("store-ip-refused"),
        "a command opened a direct connection to the store account"
    );
    ensure!(
        !probe.stdout.contains("REACHED_DIRECT") && says("direct-refused"),
        "a command opened a direct connection past the gateway"
    );
    ensure!(!says("uid=0"), "commands run as root");
    ensure!(
        !probe.stdout.contains("READ_PID1_ENV"),
        "a command read the agent's environment"
    );
    if internet {
        ensure!(says("gateway-ok"), "internet through the gateway failed");
        let code = probe
            .stdout
            .lines()
            .find_map(|l| l.strip_prefix("store-via-gateway="))
            .unwrap_or_default();
        ensure!(
            code != "200" && code != "201",
            "a command read the key's HEAD through the gateway ({code})"
        );
    } else {
        ensure!(
            !probe.stdout.contains("REACHED_OUT"),
            "a command reached out under egress none"
        );
    }
    let found: Vec<&str> = probe
        .stdout
        .lines()
        .skip_while(|l| *l != "begin-grep")
        .skip(1)
        .take_while(|l| *l != "end-grep")
        .collect();
    ensure!(found.is_empty(), "a command can read a SAS in {found:?}");

    // 3. Terminate, read without a sandbox, restore exactly.
    sandbox.terminate(&first).await.context("terminate")?;
    live.retain(|id| id != &first);
    let notes = sandbox
        .read_storage_file(key, "notes.txt")
        .await
        .context("readStorageFile")?;
    ensure!(
        notes == b"hello from the first sandbox\n",
        "readStorageFile returned {:?}",
        String::from_utf8_lossy(&notes)
    );
    let second = create(sandbox, key).await?;
    live.push(second.clone());
    let restored = manifest(sandbox, &second).await?;
    if restored != written {
        let before: BTreeMap<_, _> = written.lines().map(|l| (l.split('\t').next(), l)).collect();
        let after: BTreeMap<_, _> = restored
            .lines()
            .map(|l| (l.split('\t').next(), l))
            .collect();
        let mut diff = Vec::new();
        for (path, line) in &before {
            if after.get(path) != Some(line) {
                diff.push(format!("- {line}\n+ {:?}", after.get(path)));
            }
        }
        for (path, line) in &after {
            if !before.contains_key(path) {
                diff.push(format!("+ {line}"));
            }
        }
        bail!("the restored tree differs:\n{}", diff.join("\n"));
    }
    let integrity = ok(
        sandbox,
        &second,
        "cd /workspace && sqlite3 db.sqlite 'PRAGMA integrity_check; SELECT count(*) FROM t;' && git -C repo fsck --full && git -C repo log --oneline | wc -l && test ! -e tmp/scratch && echo excluded-absent",
    )
    .await?;
    ensure!(
        integrity.contains("ok\n5000\n") && integrity.contains("2\nexcluded-absent"),
        "restore checks: {integrity}"
    );
    eprintln!("restored {} entries exactly", restored.lines().count());

    // 4. A stale writer: a third sandbox takes the key over; the second is stopped and its later
    //    write never reaches the key.
    let third = create(sandbox, key).await?;
    live.push(third.clone());
    wait_for_state(sandbox, &second, SandboxStorageState::Stopped)
        .await
        .context("the superseded sandbox reports stopped")?;
    ok(sandbox, &second, "echo stale > /workspace/stale.txt").await?;
    let stale = sandbox.sync_storage(&second).await;
    eprintln!("stale writer's sync: {stale:?}");
    if let Ok(status) = &stale {
        ensure!(
            status.state == SandboxStorageState::Stopped,
            "a stale writer's sync answered {status:?}"
        );
    }
    sandbox.terminate(&second).await?;
    live.retain(|id| id != &second);
    sandbox.terminate(&third).await?;
    live.retain(|id| id != &third);
    match sandbox.read_storage_file(key, "stale.txt").await {
        Err(_) => {}
        Ok(bytes) => bail!(
            "the stale writer's file reached the key: {:?}",
            String::from_utf8_lossy(&bytes)
        ),
    }

    // 5. Erase the key.
    sandbox.delete_storage(key).await.context("deleteStorage")?;
    ensure!(
        sandbox.read_storage_file(key, "notes.txt").await.is_err(),
        "the erased key still reads"
    );
    Ok(())
}
