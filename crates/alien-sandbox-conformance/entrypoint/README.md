# Image boot and command identity

Build this fixture with Docker target `root`, `nonroot`, or `no-user`, then pass
that image through the backend's normal Alien build and deployment path. The
`no-user` target inherits Ubuntu's absent USER and must have root semantics.

The image supervisor records its identity and all five capability sets, tries a
real tmpfs mount and a loopback MTU change, and starts a uid-2000 child when root.
Each successful mutation is undone. The MTU operation uses an existing interface:
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
