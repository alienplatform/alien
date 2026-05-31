# StackSummary

## Example Usage

```typescript
import { StackSummary } from "@alienplatform/platform-api/models";

let value: StackSummary = {
  platforms: [],
  resourceCounts: {
    workers: 753801,
    containers: 86892,
    publicHttpsEndpoints: 224741,
    externalInfra: 58173,
    total: 40735,
  },
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `platforms`                                                        | [models.StackSummaryPlatform](../models/stacksummaryplatform.md)[] | :heavy_check_mark:                                                 | Platforms supported by the active release                          |
| `resourceCounts`                                                   | [models.ResourceCounts](../models/resourcecounts.md)               | :heavy_check_mark:                                                 | N/A                                                                |