# SandboxHeartbeatDataKubernetesPods

Kubernetes: pods carrying the sandbox label, in the deployment's namespace.

## Example Usage

```typescript
import { SandboxHeartbeatDataKubernetesPods } from "@alienplatform/manager-api/models";

let value: SandboxHeartbeatDataKubernetesPods = {
  activeSessions: 169819,
  idlePods: 607989,
  namespace: "<value>",
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "unknown",
    partial: true,
    stale: false,
  },
  backend: "kubernetesPods",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `activeSessions`                                                     | *number*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `idlePods`                                                           | *number*                                                             | :heavy_check_mark:                                                   | Claimed but unused pods waiting in the pool.                         |
| `namespace`                                                          | *string*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `status`                                                             | [models.SandboxHeartbeatStatus](../models/sandboxheartbeatstatus.md) | :heavy_check_mark:                                                   | N/A                                                                  |
| `backend`                                                            | *"kubernetesPods"*                                                   | :heavy_check_mark:                                                   | N/A                                                                  |