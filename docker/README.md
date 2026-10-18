# Container Images

Canonical Dockerfiles for OSS image builds:

- `Dockerfile.alien-base`
- `Dockerfile.alien-builder`

Images are built and published by GitHub Actions workflows in `.github/workflows/images*.yml`.

## Sandbox command identity

AWS bundles and layered GCP sandbox images resolve the base image's OCI `USER` against
that image's `/etc/passwd` and `/etc/group`. A non-root user keeps its numeric uid and
primary gid, including `USER name:group` and numeric forms. Existing account entries,
home directories, and image-owned writable directories are preserved. A root or absent
`USER` uses uid/gid 60000 on AWS and 1000 on GCP. Unknown names, invalid ids, and primary
group 0 are rejected before building: commands never run as root or in the root group.
A numeric user without a passwd entry needs an explicit non-root group, for example
`USER 1001:2001`.

The agent binary and Alien-owned metadata remain root-owned and unwritable by commands.
AWS starts the agent as root and drops privileges for each command. GCP starts the agent
as the resolved non-root user because Agent Platform does not permit a root image.
`ALIEN_SANDBOX_ROOT` names the writable session directory (normally `/sandbox`), owned by
the command uid/gid. Commands also retain access to image directories permitted by their
ownership and by the backend's filesystem policy.

`ALIEN_SANDBOX_EXEC_UID` and `ALIEN_SANDBOX_EXEC_GID` carry the resolved numeric identity.
They are baked into the image and provided to commands, together with
`ALIEN_SANDBOX_ROOT`; image tooling should read these variables rather than hardcode a uid.
AWS runtime templates do not override the identity chosen during image assembly.

`privilegedSupervisor.commandUid`, while supported, must agree with a non-root base
`USER`. The bundle builder and source build reject mismatches. Root/absent-user bundles
can select that declared non-root uid. Supply `--privileged-supervisor --command-uid UID`
to the `sandbox-bundle` example when packaging one. A prebuilt S3 bundle has no inspectable
OCI user at plan time; the agent checks the declaration against its baked identity before
serving. Changing the declaration requires rebuilding the bundle.

Local Docker sessions inspect the image and resolve the same account data before creating
a container. Root/absent-user images use 65534:65534. Exec and file uploads inherit the
selected identity. The root filesystem stays read-only; `/sandbox` is a private writable
tmpfs owned by the selected uid/gid. Selecting the image user does not make `/data` writable
on a backend that mounts the root filesystem read-only.

Kubernetes currently renders externally supplied image references without fetching registry
configuration or account files. Neither the pod renderer nor its pool controller has a
base-image inspection step or registry credentials. It therefore retains the explicit
65534 identity and read-only root filesystem. Named image users cannot be safely adopted
by merely dropping `runAsUser`: Kubernetes cannot validate a named `USER` with
`runAsNonRoot`, and root/absent-user images would lose the non-root fallback. These images
must currently accommodate the Kubernetes identity. Azure's managed catalog sandboxes do
not expose a customer base image for Alien to inspect or choose an identity from.
