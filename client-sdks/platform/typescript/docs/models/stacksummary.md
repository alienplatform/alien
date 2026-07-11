# StackSummary

## Example Usage

```typescript
import { StackSummary } from "@alienplatform/platform-api/models";

let value: StackSummary = {
  platforms: [],
  requiresNetwork: false,
  resourceCounts: {
    workers: 86892,
    containers: 224741,
    publicHttpsEndpoints: 58173,
    externalInfra: 40735,
    total: 340794,
  },
  publicEndpoints: [],
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `platforms`                                                            | [models.StackSummaryPlatform](../models/stacksummaryplatform.md)[]     | :heavy_check_mark:                                                     | Platforms supported by the active release                              |
| `requiresNetwork`                                                      | *boolean*                                                              | :heavy_check_mark:                                                     | Whether the stack contains resources that require cloud VPC networking |
| `resourceCounts`                                                       | [models.ResourceCounts](../models/resourcecounts.md)                   | :heavy_check_mark:                                                     | N/A                                                                    |
| `publicEndpoints`                                                      | [models.PublicEndpoint](../models/publicendpoint.md)[]                 | :heavy_check_mark:                                                     | Public endpoints declared by the active release stack                  |