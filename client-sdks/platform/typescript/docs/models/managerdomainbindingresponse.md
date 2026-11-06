# ManagerDomainBindingResponse

## Example Usage

```typescript
import { ManagerDomainBindingResponse } from "@alienplatform/platform-api/models";

let value: ManagerDomainBindingResponse = {
  managerDomainBinding: {
    id: "dend_1bb6gdvm1bs74acqkjstcgvxnc82",
    workspaceId: "ws_It13CUaGEhLLAB87simX07Xb",
    domainId: "dom_469m0agk8luj4s16sakmmpddz07x",
    kind: "deployment_portal",
    owner: {
      type: "manager",
      id: "<id>",
    },
    hostname: "different-slipper.info",
    status: "waiting_for_domain",
    managedDnsRecords: [],
    retryAttempts: 645201,
    createdAt: new Date("2026-02-13T20:08:55.887Z"),
    updatedAt: new Date("2024-08-10T13:36:22.642Z"),
  },
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `managerDomainBinding`                               | [models.DomainEndpoint](../models/domainendpoint.md) | :heavy_check_mark:                                   | N/A                                                  |