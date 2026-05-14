# StackSummary

## Example Usage

```typescript
import { StackSummary } from "@alienplatform/platform-api/models";

let value: StackSummary = {
  platforms: [],
  resourceCounts: {
    functions: 753801,
    containers: 86892,
    externalInfra: 224741,
    total: 58173,
  },
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `platforms`                                                        | [models.StackSummaryPlatform](../models/stacksummaryplatform.md)[] | :heavy_check_mark:                                                 | Platforms supported by the active release                          |
| `resourceCounts`                                                   | [models.ResourceCounts](../models/resourcecounts.md)               | :heavy_check_mark:                                                 | N/A                                                                |