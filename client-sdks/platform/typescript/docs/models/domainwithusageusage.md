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
      hostname: "prudent-duffel.net",
    },
  ],
  packageDomains: [
    {
      id: "<id>",
      hostname: "prickly-role.biz",
    },
  ],
  managerBindings: [
    {
      id: "<id>",
      managerId: "<id>",
      managerName: "<value>",
      hostname: "adolescent-space.name",
    },
  ],
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `deploymentUrlProjects`                                                            | [models.DeploymentUrlProject](../models/deploymenturlproject.md)[]                 | :heavy_check_mark:                                                                 | N/A                                                                                |
| `portalBindings`                                                                   | [models.PortalBinding](../models/portalbinding.md)[]                               | :heavy_check_mark:                                                                 | N/A                                                                                |
| `packageDomains`                                                                   | [models.DomainWithUsagePackageDomain](../models/domainwithusagepackagedomain.md)[] | :heavy_check_mark:                                                                 | N/A                                                                                |
| `managerBindings`                                                                  | [models.ManagerBinding](../models/managerbinding.md)[]                             | :heavy_check_mark:                                                                 | N/A                                                                                |