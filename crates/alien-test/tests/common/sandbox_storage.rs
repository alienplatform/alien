//! Checks for the `sandbox-storage` app: a remote sandbox whose `/workspace` is persistent
//! storage. The test drives the deployment's own binding, as a customer's code would: it writes a
//! tree, syncs, terminates, restores the key into a new sandbox, reads a file without a sandbox,
//! has a stale writer refused and erases the key.

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
    let created = sandbox
        .create(CreateSandboxRequest {
            storage_key: Some(key.to_string()),
            ..Default::default()
        })
        .await
        .with_context(|| format!("create a sandbox for key {key}"))?;
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

/// The whole persistent storage path through the deployment's remote binding.
pub async fn check_persistent_storage(
    deployment: &alien_test::TestDeployment,
    platform: Platform,
) -> anyhow::Result<()> {
    let discovery = super::remote_bindings::DiscoveryServer::start(deployment, platform).await?;
    let bindings = alien_bindings::RemoteBindings::for_deployment(
        &deployment.id,
        &deployment.token,
        Some(&discovery.url),
    )
    .await
    .context("discover assigned manager for remote bindings")?;
    let sandbox: Arc<dyn Sandbox> = bindings
        .sandbox("agents")
        .await
        .context("resolve the remote sandbox")?;
    let capabilities = sandbox.capabilities();
    ensure!(
        capabilities.persistent_storage,
        "the capability table does not report persistentStorage: {capabilities:?}"
    );

    let key = format!("e2e-{}", chrono::Utc::now().timestamp_millis());
    let mut live: Vec<String> = Vec::new();
    let result = run_checks(&sandbox, &key, &mut live).await;
    for id in live {
        if let Err(error) = sandbox.terminate(&id).await {
            eprintln!("cleanup: terminate {id}: {error}");
        }
    }
    if let Err(error) = sandbox.delete_storage(&key).await {
        eprintln!("cleanup: delete key {key}: {error}");
    }
    result
}

async fn run_checks(
    sandbox: &Arc<dyn Sandbox>,
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

    // 2. The command's uid sees no rights and reaches nothing outside.
    let probe = sh(
        sandbox,
        &first,
        r#"id -u; env | grep -c 'sig=' || true; cat /proc/1/environ >/dev/null 2>&1 && echo READ_PID1_ENV || echo pid1-env-denied; echo begin-grep;
grep -rls 'sig=' /proc/[0-9]*/environ /proc/[0-9]*/cmdline /tmp /workspace /opt/alien 2>/dev/null | head -5; echo end-grep;
curl -sS -m 8 -o /dev/null https://management.azure.com/ && echo REACHED_OUT || echo outbound-refused;
curl -sS -m 8 -o /dev/null https://www.microsoft.com/ && echo REACHED_OUT || echo outbound-refused"#,
    )
    .await?;
    eprintln!("command-uid probe:\n{}{}", probe.stdout, probe.stderr);
    ensure!(
        probe.stdout.lines().next() != Some("0"),
        "commands run as root"
    );
    ensure!(
        !probe.stdout.contains("READ_PID1_ENV"),
        "a command read the agent's environment"
    );
    ensure!(
        !probe.stdout.contains("REACHED_OUT"),
        "a command reached out under egress none"
    );
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
