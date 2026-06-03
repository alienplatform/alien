# ManagerDomainBindingResponse

## Example Usage

```typescript
import { ManagerDomainBindingResponse } from "@alienplatform/platform-api/models";

let value: ManagerDomainBindingResponse = {
  managerDomainBinding: {
    id: "mdb_crarp1rv29ef40gdifx8kiac",
    workspaceId: "ws_It13CUaGEhLLAB87simX0",
    managerId: "mgr_enxscjrqiiu2lrc672hwwuc5",
    domainId: "dom_469m0agk8luj4s16sakmmpdd",
    hostname: "colorful-cope.net",
    status: "pending-endpoint",
    retryAttempts: 102654,
    createdAt: new Date("2024-02-10T02:09:23.400Z"),
    updatedAt: new Date("2025-12-08T03:21:00.870Z"),
  },
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `managerDomainBinding`                                           | [models.ManagerDomainBinding](../models/managerdomainbinding.md) | :heavy_check_mark:                                               | N/A                                                              |