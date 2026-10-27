# Deployment-time container resources

Use fixed CPU and memory allocations when every deployment has the same workload.
Use ranges when an installer should choose resources without publishing another release:

```ts
const api = new Container("api")
  .cpu({ min: 0.5, max: 4, default: 1 })
  .memory({ min: "512Mi", max: "8Gi", default: "2Gi" })
```

Select allocations when creating or preparing a deployment:

```ts
const settings = {
  compute: {
    containers: {
      api: { cpu: 2, memory: "4Gi" },
    },
    pools: {
      general: {
        mode: "autoscale",
        min: 1,
        max: 5,
      },
    },
  },
}
```

CPU is in vCPUs; memory uses binary units (`Ki`, `Mi`, `Gi`, `Ti`). Omitted
selections use the release's defaults. Fixed `.cpu(1)` and `.memory("2Gi")`
declarations remain fixed. Unknown containers and allocations outside the declared
ranges are rejected before provisioning workloads.

Container allocations are portable across AWS, GCP, Azure, and Kubernetes.
Pool machine names are provider-specific: use an EC2 instance type, a GCE machine
type, or an Azure VM SKU. Compute planning uses the selected container allocations
to recommend suitable machines and validate fleet capacity.

Omit `machine` to let Alien choose it from the selected resources on setup and
later updates. Set `machine` to a provider machine name to override that choice;
remove the override to return to automatic sizing. A resource update can replace
the machine while keeping the pool's count policy and persistent volumes.
Single-replica stateful workloads may restart during replacement. An explicit
machine that cannot fit the allocation is rejected before cloud changes.

These are deployment choices, not vertical autoscaling. Each replica receives the
selected allocation. An autoscaling machine pool adds capacity when replicas need
it, within the pool's minimum and maximum. A fixed pool cannot add machines, and
adding machines cannot make a single replica fit on an undersized machine.

The same resource choices work for stateful containers. Choosing CPU and memory
does not change persistent disk size or enable horizontal autoscaling of stateful
replicas.

CloudFormation exposes declared choices as parameters such as `ContainerApiCpu`
and `ContainerApiMemory`. Terraform and API deployments carry selections in
deployment settings. The deployment portal exposes the declared ranges and
replans machine choices when allocations change.

### Conditional inputs

Use `when` for inputs that only apply while another boolean input has a given value:

```ts
const io = alien.inputs({
  snapshotsEnabled: alien.boolean({
    providedBy: "deployer", required: true, default: true,
    label: "Snapshots enabled", description: "Keep periodic snapshots.",
  }),
  snapshotBucket: alien.string({
    providedBy: "deployer", required: true,
    label: "Snapshot bucket", description: "Bucket for snapshots.",
    when: { input: "snapshotsEnabled", equals: true },
    env: "SNAPSHOT_BUCKET",
  }),
})
```

The condition reads the provided boolean, otherwise its default. Inactive inputs
are not asked for, required, validated or mapped into environment variables.
Provided values for inactive inputs are ignored, so callers can submit full forms.
When active, the input's normal requiredness and validation apply.

The referenced input must be a boolean in the same stack and must not have its own
`when`. Self-references, chains and cycles are rejected. A conditional boolean
cannot gate a resource with `.enabled()`. A deployer-provided input requires a
controller that can also be provided by the deployer; developer-only controllers
may control developer-only inputs. `when` cannot be combined with `generate`.
An input excluded from the deployment platform does not apply. An off-platform
controller resolves to false, so `equals: false` matches and `equals: true` does
not. An unanswered on-platform reference matches neither value.

Both the CLI and manager must include conditional-input support before releasing
or installing a stack that uses `when`. Version 3.3.43 predates that support;
until a supporting release is tagged, build both from the revision introducing
this feature or newer. Older CLIs and managers can require inactive inputs.
Hosted management must also have conditional-input support deployed before any
stack using `when` is released.

Terraform modules using conditional inputs require Terraform 1.9 or newer.
CloudFormation uses Rules for conditional requiredness; other constraints are
validated during registration because CloudFormation parameter constraints apply
even when an input is inactive.

CloudFormation does not evaluate Rules when creating a CREATE change set. It
checks them during stack creation: a failed assertion rolls the stack back before
creating resources. Validate both condition states with CreateStack, not only
CreateChangeSet.
