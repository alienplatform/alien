# DeploymentPortalDomainResponse

## Example Usage

```typescript
import { DeploymentPortalDomainResponse } from "@alienplatform/platform-api/models";

let value: DeploymentPortalDomainResponse = {
  deploymentPortalEndpoint: {
    id: "dend_1bb6gdvm1bs74acqkjstcgv",
    workspaceId: "ws_It13CUaGEhLLAB87simX0",
    domainId: "dom_469m0agk8luj4s16sakmmpdd",
    kind: "workspace_packages",
    owner: {
      type: "manager",
      id: "<id>",
    },
    hostname: "twin-disk.biz",
    status: "waiting_for_dns",
    managedDnsRecords: [
      {
        name: "<value>",
        type: "<value>",
        value: "<value>",
      },
    ],
    retryAttempts: 455485,
    createdAt: new Date("2025-04-09T18:24:23.976Z"),
    updatedAt: new Date("2025-01-30T06:43:41.751Z"),
  },
  packageEndpoint: {
    id: "dend_1bb6gdvm1bs74acqkjstcgv",
    workspaceId: "ws_It13CUaGEhLLAB87simX0",
    domainId: "dom_469m0agk8luj4s16sakmmpdd",
    kind: "manager_api",
    owner: {
      type: "manager",
      id: "<id>",
    },
    hostname: "lively-injunction.com",
    status: "waiting_for_health",
    managedDnsRecords: [
      {
        name: "<value>",
        type: "<value>",
        value: "<value>",
      },
    ],
    retryAttempts: 561042,
    createdAt: new Date("2024-05-17T20:42:56.030Z"),
    updatedAt: new Date("2026-03-18T20:42:00.688Z"),
  },
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `deploymentPortalEndpoint`                           | [models.DomainEndpoint](../models/domainendpoint.md) | :heavy_check_mark:                                   | N/A                                                  |
| `packageEndpoint`                                    | [models.DomainEndpoint](../models/domainendpoint.md) | :heavy_check_mark:                                   | N/A                                                  |