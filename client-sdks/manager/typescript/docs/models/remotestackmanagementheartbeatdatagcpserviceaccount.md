# RemoteStackManagementHeartbeatDataGcpServiceAccount

## Example Usage

```typescript
import { RemoteStackManagementHeartbeatDataGcpServiceAccount } from "@alienplatform/manager-api/models";

let value: RemoteStackManagementHeartbeatDataGcpServiceAccount = {
  impersonationGranted: true,
  roleBound: true,
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "running",
    partial: true,
    stale: true,
  },
  backend: "gcpServiceAccount",
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `impersonationGranted`                                                                           | *boolean*                                                                                        | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `roleBound`                                                                                      | *boolean*                                                                                        | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `serviceAccountEmail`                                                                            | *string*                                                                                         | :heavy_minus_sign:                                                                               | N/A                                                                                              |
| `serviceAccountUniqueId`                                                                         | *string*                                                                                         | :heavy_minus_sign:                                                                               | N/A                                                                                              |
| `status`                                                                                         | [models.RemoteStackManagementHeartbeatStatus](../models/remotestackmanagementheartbeatstatus.md) | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `backend`                                                                                        | *"gcpServiceAccount"*                                                                            | :heavy_check_mark:                                                                               | N/A                                                                                              |