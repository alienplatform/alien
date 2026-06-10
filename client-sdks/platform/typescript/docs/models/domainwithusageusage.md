# DomainWithUsageUsage

## Example Usage

```typescript
import { DomainWithUsageUsage } from "@alienplatform/platform-api/models";

let value: DomainWithUsageUsage = {
  deploymentUrlProjects: [],
  portalBindings: [
    {
      id: "<id>",
      projectId: "<id>",
      projectName: "<value>",
      hostname: "superior-mathematics.name",
    },
  ],
  packageDomains: [
    {
      id: "<id>",
      hostname: "faint-submitter.com",
    },
  ],
  managerBindings: [
    {
      id: "<id>",
      managerId: "<id>",
      managerName: "<value>",
      hostname: "neighboring-vestment.biz",
    },
  ],
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `deploymentUrlProjects`                                            | [models.DeploymentUrlProject](../models/deploymenturlproject.md)[] | :heavy_check_mark:                                                 | N/A                                                                |
| `portalBindings`                                                   | [models.PortalBinding](../models/portalbinding.md)[]               | :heavy_check_mark:                                                 | N/A                                                                |
| `packageDomains`                                                   | [models.PackageDomain](../models/packagedomain.md)[]               | :heavy_check_mark:                                                 | N/A                                                                |
| `managerBindings`                                                  | [models.ManagerBinding](../models/managerbinding.md)[]             | :heavy_check_mark:                                                 | N/A                                                                |