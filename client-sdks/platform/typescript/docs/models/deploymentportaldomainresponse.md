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
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `deploymentPortalDomain`                                             | [models.DeploymentPortalDomain](../models/deploymentportaldomain.md) | :heavy_check_mark:                                                   | N/A                                                                  |