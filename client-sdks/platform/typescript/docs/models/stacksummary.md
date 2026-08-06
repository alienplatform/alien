# StackSummary

## Example Usage

```typescript
import { StackSummary } from "@alienplatform/platform-api/models";

let value: StackSummary = {
  platforms: [],
  requiresNetwork: false,
  customerModels: true,
  resourceCounts: {
    workers: 224741,
    containers: 58173,
    publicHttpsEndpoints: 40735,
    externalInfra: 340794,
    total: 6792,
  },
  publicEndpoints: [
    {
      resourceId: "<id>",
      endpointName: "<value>",
      hostLabel: "<value>",
      wildcardSubdomains: true,
    },
  ],
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `platforms`                                                            | [models.StackSummaryPlatform](../models/stacksummaryplatform.md)[]     | :heavy_check_mark:                                                     | Platforms supported by the active release                              |
| `requiresNetwork`                                                      | *boolean*                                                              | :heavy_check_mark:                                                     | Whether the stack contains resources that require cloud VPC networking |
| `customerModels`                                                       | *boolean*                                                              | :heavy_check_mark:                                                     | Whether this release offers one remotely accessible AI resource        |
| `resourceCounts`                                                       | [models.ResourceCounts](../models/resourcecounts.md)                   | :heavy_check_mark:                                                     | N/A                                                                    |
| `publicEndpoints`                                                      | [models.PublicEndpoint](../models/publicendpoint.md)[]                 | :heavy_check_mark:                                                     | Public endpoints declared by the active release stack                  |