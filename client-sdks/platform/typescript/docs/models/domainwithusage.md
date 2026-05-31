# DomainWithUsage

## Example Usage

```typescript
import { DomainWithUsage } from "@alienplatform/platform-api/models";

let value: DomainWithUsage = {
  id: "dom_469m0agk8luj4s16sakmmpdd",
  workspaceId: "ws_It13CUaGEhLLAB87simX0",
  domain: "free-antelope.name",
  isSystem: false,
  claimToken: "<value>",
  status: "pending-verification",
  createdAt: new Date("2025-12-13T18:16:52.351Z"),
  updatedAt: new Date("2025-02-11T22:17:17.700Z"),
  usage: {
    deploymentUrlProjects: [],
    portalBindings: [],
  },
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `id`                                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the domain.                                                             | dom_469m0agk8luj4s16sakmmpdd                                                                  |
| `workspaceId`                                                                                 | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the workspace.                                                          | ws_It13CUaGEhLLAB87simX0                                                                      |
| `domain`                                                                                      | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `isSystem`                                                                                    | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `claimToken`                                                                                  | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `hostedZoneId`                                                                                | *string*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |                                                                                               |
| `nameServers`                                                                                 | *string*[]                                                                                    | :heavy_minus_sign:                                                                            | N/A                                                                                           |                                                                                               |
| `status`                                                                                      | [models.DomainWithUsageStatus](../models/domainwithusagestatus.md)                            | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `error`                                                                                       | *any*                                                                                         | :heavy_minus_sign:                                                                            | N/A                                                                                           |                                                                                               |
| `createdAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `updatedAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `verifiedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | N/A                                                                                           |                                                                                               |
| `usage`                                                                                       | [models.DomainWithUsageUsage](../models/domainwithusageusage.md)                              | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |