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
    status: "pending-edge",
    managedDnsRecords: [],
    retryAttempts: 36578,
    createdAt: new Date("2025-12-08T03:21:00.870Z"),
    updatedAt: new Date("2026-02-13T20:08:55.887Z"),
  },
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `managerDomainBinding`                                           | [models.ManagerDomainBinding](../models/managerdomainbinding.md) | :heavy_check_mark:                                               | N/A                                                              |