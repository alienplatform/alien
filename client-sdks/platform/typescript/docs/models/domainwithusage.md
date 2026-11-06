# DomainWithUsage

## Example Usage

```typescript
import { DomainWithUsage } from "@alienplatform/platform-api/models";

let value: DomainWithUsage = {
  id: "dom_469m0agk8luj4s16sakmmpddz07x",
  workspaceId: "ws_It13CUaGEhLLAB87simX07Xb",
  domain: "free-antelope.name",
  dns: "customer",
  isSystem: true,
  claimToken: "<value>",
  status: "lost-verification",
  createdAt: new Date("2025-02-11T22:17:17.700Z"),
  updatedAt: new Date("2024-07-21T08:54:10.474Z"),
  endpoints: [],
  usage: {
    deploymentUrlProjects: [],
    portalBindings: [
      {
        id: "<id>",
        projectId: "<id>",
        projectName: "<value>",
        hostname: "superior-mathematics.name",
      },
    ],
    packageDomains: [],
    managerBindings: [],
  },
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `id`                                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the domain.                                                             | dom_469m0agk8luj4s16sakmmpddz07x                                                              |
| `workspaceId`                                                                                 | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the workspace.                                                          | ws_It13CUaGEhLLAB87simX07Xb                                                                   |
| `domain`                                                                                      | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `dns`                                                                                         | [models.Dns](../models/dns.md)                                                                | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `acmeChallengeTarget`                                                                         | *string*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |                                                                                               |
| `isSystem`                                                                                    | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `claimToken`                                                                                  | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `hostedZoneId`                                                                                | *string*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |                                                                                               |
| `nameServers`                                                                                 | *string*[]                                                                                    | :heavy_minus_sign:                                                                            | N/A                                                                                           |                                                                                               |
| `status`                                                                                      | [models.DomainWithUsageStatus](../models/domainwithusagestatus.md)                            | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `error`                                                                                       | *any*                                                                                         | :heavy_minus_sign:                                                                            | N/A                                                                                           |                                                                                               |
| `createdAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `updatedAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `verifiedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | N/A                                                                                           |                                                                                               |
| `endpoints`                                                                                   | [models.DomainEndpoint](../models/domainendpoint.md)[]                                        | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `usage`                                                                                       | [models.DomainWithUsageUsage](../models/domainwithusageusage.md)                              | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |