# Image boot and command identity

Build this fixture with Docker target `root`, `nonroot`, or `no-user`, then pass
that image through the backend's normal Alien build and deployment path. The
`no-user` target inherits Ubuntu's absent USER and must have root semantics.

The image supervisor records its identity and all five capability sets, tries a
real tmpfs mount and a loopback MTU change, and starts a uid-2000 child when root.
Each successful mutation is undone. MTU updates use RTM_NEWLINK because gVisor
does not implement SIOCSIFMTU. The no_new_privs check uses prctl because gVisor
omits the corresponding procfs field. The MTU operation uses an existing interface:
a kernel without the dummy-link driver can reject `ip link add ... type dummy`
even when NET_ADMIN is available, which is not evidence of a capability denial.

Use this as the first binding exec, with the expected backend command uid:

```sh
python3 /opt/entrypoint-conformance/inspect.py --image-uid 0 --exec-uid 60000
```

Use `--image-uid 2001` for the non-root target. The check records whether the
supervisor exists at first exec, waits for its report, and requires exactly one
start, the expected image identity, root capabilities matching Docker defaults
without NET_RAW, denied SYS_ADMIN/NET_ADMIN operations, and a non-root exec with
no capabilities and `no_new_privs`. It prints observed evidence before assertions.

NET_RAW is excluded so root gets the same capability set on GCP, which removes
it from its default grant. Raw sockets are not required for uid changes or the
supervisor setup this fixture exercises.

The fixture does not by itself prove egress containment, lifecycle health, or
resume behavior. Run it through each real backend and pair network probes with
reachable controls. A provider returning a fresh container on resume has a new
boot; a memory resume must retain its existing image process.

An Azure native-image baseline on 2026-10-22 ran both this kind of root image
entrypoint and native exec as uid 0 with all 41 Linux capabilities, and permitted
tmpfs mounts and loopback MTU changes. Agent packaging and exec privilege drops
are therefore required to satisfy this contract there.

For cloud metadata qualification, build with `--build-arg METADATA_BACKEND=aws`
(or `gcp` / `azure`) and pass `--metadata-backend aws` to the first exec checker.
The boot process, its uid-2000 child and exec probe metadata independently. Requests
bypass proxy environment variables, use the provider's required headers, and cover
IPv4, both documented cloud IPv6 metadata addresses, and the GCP metadata hostname.
AWS probes both IMDSv1 and IMDSv2, including the role and instance-identity credential
paths. The IPv6 addresses are attempted on every backend; this does not claim that
every provider implements both addresses.

Reports retain status, response size, transport error type, and the known AWS MMDS
`placement/` and `tags/` directory names. They never print response bodies or tokens.
A successful credential endpoint, redirect, unfamiliar status, or unfamiliar MMDS
directory fails the checker. An HTTP refusal or inability to connect establishes
only that this request obtained no credentials; it does not prove the absence of
other metadata paths or a permanent network policy. Save these observations beside
the external-boundary and provider-configuration evidence.

Run the local HTTP response controls with `python3 -m unittest test_metadata` in
this directory. These controls verify that credential responses fail without
leaking their contents; they do not replace live cloud qualification.

A GCP Agent Platform native-image baseline on 2026-10-22, with internet access
enabled, passed this checker: root USER retained exactly `0xa80405fb`, the child
ran as uid 2000, and both mount and RTM_NEWLINK were refused with EPERM. Root,
uid 2000, and capless uid 60000 obtained no metadata credentials: IPv4 requests
timed out, IPv6 had no route, and `metadata.google.internal` did not resolve.
This baseline used a probe exec adapter; it does not qualify Alien packaging.

An Azure native-image baseline on 2026-10-22 with Allow egress obtained no metadata
credentials as root ENTRYPOINT, its uid-2000 child, native root exec, or a capless
uid-60000 exec. IPv4 returned ECONNREFUSED and IPv6 EAFNOSUPPORT. Native root still
had all 41 capabilities and could mount/change MTU, so this only qualifies metadata
exclusion; Alien agent packaging must supply the image and exec privilege bounds.
