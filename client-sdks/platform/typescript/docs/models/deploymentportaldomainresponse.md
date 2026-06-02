# DeploymentPortalDomainResponse

## Example Usage

```typescript
import { DeploymentPortalDomainResponse } from "@alienplatform/platform-api/models";

let value: DeploymentPortalDomainResponse = {
  deploymentPortalDomain: {
    id: "dpd_56cfoqypfvtmlq2f6m54t33y",
    workspaceId: "ws_It13CUaGEhLLAB87simX0",
    projectId: "prj_mcytp6z3j91f7tn5ryqsfwtr",
    domainId: "dom_469m0agk8luj4s16sakmmpdd",
    hostname: "lined-stay.org",
    status: "pending-vercel",
    managedDnsRecords: [],
    retryAttempts: 327945,
    createdAt: new Date("2025-09-14T17:52:57.276Z"),
    updatedAt: new Date("2025-05-14T05:04:01.010Z"),
  },
  packageDomain: {
    id: "pkgdom_9mgov33m1tfr2a3v80csx",
    workspaceId: "ws_It13CUaGEhLLAB87simX0",
    domainId: "dom_469m0agk8luj4s16sakmmpdd",
    hostname: "haunting-tomatillo.name",
    status: "pending-health",
    managedDnsRecords: [],
    retryAttempts: 121408,
    createdAt: new Date("2025-04-16T03:33:15.160Z"),
    updatedAt: new Date("2025-09-25T10:57:15.624Z"),
  },
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `deploymentPortalDomain`                                             | [models.DeploymentPortalDomain](../models/deploymentportaldomain.md) | :heavy_check_mark:                                                   | N/A                                                                  |
| `packageDomain`                                                      | [models.PackageDomain](../models/packagedomain.md)                   | :heavy_check_mark:                                                   | N/A                                                                  |