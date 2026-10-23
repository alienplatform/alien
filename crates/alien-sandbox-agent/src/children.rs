//! Reap adopted children without consuming the exit status of a managed command.

use std::{collections::HashSet, io, sync::Mutex, time::Duration};
use tokio::process::{Child, Command};

static MANAGED: Mutex<Option<HashSet<u32>>> = Mutex::new(None);

/// Keep this registration alive until the command's waiter finishes or is dropped.
#[derive(Debug)]
pub struct Registration(u32);

impl Drop for Registration {
    fn drop(&mut self) {
        // A poisoned registry is fatal to the reaper too; never guess which children
        // are safe to collect after a panic during spawn.
        if let Ok(mut managed) = MANAGED.lock() {
            if let Some(managed) = managed.as_mut() {
                managed.remove(&self.0);
            }
        }
    }
}

pub fn spawn(command: &mut Command) -> io::Result<(Child, Registration)> {
    let mut managed = MANAGED
        .lock()
        .map_err(|_| io::Error::other("child registry poisoned"))?;
    // Serialize spawn and registration with reaping: even a child that immediately
    // exits must not be mistaken for an adopted orphan.
    let child = command.spawn()?;
    let pid = child
        .id()
        .ok_or_else(|| io::Error::other("spawn returned no child PID"))?;
    managed.get_or_insert_with(HashSet::new).insert(pid);
    Ok((child, Registration(pid)))
}

#[cfg(target_os = "linux")]
fn reap() -> io::Result<()> {
    let managed = MANAGED
        .lock()
        .map_err(|_| io::Error::other("child registry poisoned"))?;
    // Linux associates direct children with the spawning thread. Adopted children
    // can belong to another thread, so enumerate every thread's children.
    for task in std::fs::read_dir("/proc/self/task")? {
        let path = task?.path().join("children");
        let children = match std::fs::read_to_string(path) {
            Ok(children) => children,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        for child in children.split_whitespace() {
            let pid: u32 = child.parse().map_err(io::Error::other)?;
            if managed
                .as_ref()
                .is_some_and(|managed| managed.contains(&pid))
            {
                continue;
            }
            // Target only this unregistered child. ECHILD is expected if Tokio's
            // abandoned-child collector got there first. Live orphans return zero.
            if unsafe { libc::waitpid(pid as i32, std::ptr::null_mut(), libc::WNOHANG) } < 0 {
                let error = io::Error::last_os_error();
                if !matches!(error.raw_os_error(), Some(libc::ECHILD | libc::EINTR)) {
                    return Err(error);
                }
            }
        }
    }
    Ok(())
}

/// PID 1 owns abandoned descendants; other agents leave them to the host init.
pub async fn reap_orphans() -> io::Result<()> {
    #[cfg(target_os = "linux")]
    if std::process::id() == 1 {
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        loop {
            interval.tick().await;
            reap()?;
        }
    }
    std::future::pending().await
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn adopted_children_are_reaped_without_stealing_command_status() {
        const CHILD: &str = "ALIEN_TEST_ORPHAN_REAPER";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "children::tests::adopted_children_are_reaped_without_stealing_command_status",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        // Confine adoption to this test subprocess; no privileges or global PID
        // namespace changes are required to exercise the same kernel behavior.
        assert_eq!(
            unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) },
            0
        );
        let temp = tempfile::tempdir().unwrap();
        let mut command = Command::new("/bin/sh");
        command
            .current_dir(temp.path())
            .args(["-c", "sleep 0.1 & echo $! > orphan; exit 37"]);
        let (mut child, _registration) = spawn(&mut command).unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        reap().unwrap();
        assert_eq!(child.wait().await.unwrap().code(), Some(37));
        let orphan = std::fs::read_to_string(temp.path().join("orphan")).unwrap();
        let proc = format!("/proc/{}", orphan.trim());
        tokio::time::timeout(Duration::from_secs(3), async {
            while std::path::Path::new(&proc).exists() {
                reap().unwrap();
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("adopted zombie must disappear");
    }
}
