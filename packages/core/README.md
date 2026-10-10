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
        machine: "m7g.large",
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
