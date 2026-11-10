# Helm setup update e2e

Runs the Helm setup update flow on a local kind cluster with the SQLite
manager: a release adds a frozen resource, the deployment pauses, and an
ordinary `helm upgrade` (or `helm template | kubectl apply`) to the chart for
that release completes the update.

Every chart is the one the manager serves for a release, with the setup it
carries; only the install-channel case rewrites the setup target.

Cases, each asserted against the deployment the manager stores:

- a retry without a chart upgrade leaves the update waiting and mints nothing;
- a chart for an older release than the one waiting is refused, with the
  release to use;
- a chart for a release that adds a cloud resource is refused (cloud setup
  needed);
- the chart for the waiting release completes the update (`helm upgrade`);
- a chart-only version bump with the same setup mints nothing;
- `helm rollback` after success mints nothing and the deployment keeps running;
- a chart built for another install channel is refused;
- a rollback, after the grant was minted, to a chart with a different setup
  leaves the update waiting, and the upgrade to the right chart completes it;
- `helm template --no-hooks | kubectl apply` completes the update;
- the older-Helm fallback (`helm get values -o yaml > current.yaml`, then
  `-f current.yaml`) completes the update.

## Requirements

`docker`, `kind`, `kubectl`, `helm` (3.14 or newer), `curl`, `python3`, and
built `alien-manager` and `alien-operator` binaries (Linux, same architecture
as the host):

```sh
cargo build -p alien-manager -p alien-operator
tests/e2e/helm-setup-update/run.sh
```

Environment: `ALIEN_MANAGER_BIN`, `ALIEN_OPERATOR_BIN` (default
`target/debug/...`), `KIND_CLUSTER` (default `helm-setup-update`; the script exits if a cluster
with that name already exists, and deletes only a cluster it created),
`KEEP=1` to keep the cluster and manager after the run. The script builds an
operator image, loads it into the cluster, and removes everything it created
unless `KEEP=1`. It prints `PASS`/`FAIL` per case and exits non-zero on the
first failure; the manager log is in the work directory it prints.
