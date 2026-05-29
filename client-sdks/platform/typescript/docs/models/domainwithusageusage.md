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
      hostname: "prudent-duffel.net",
    },
  ],
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `deploymentUrlProjects`                                            | [models.DeploymentUrlProject](../models/deploymenturlproject.md)[] | :heavy_check_mark:                                                 | N/A                                                                |
| `portalBindings`                                                   | [models.PortalBinding](../models/portalbinding.md)[]               | :heavy_check_mark:                                                 | N/A                                                                |