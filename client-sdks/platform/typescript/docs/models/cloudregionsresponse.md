# CloudRegionsResponse

## Example Usage

```typescript
import { CloudRegionsResponse } from "@alienplatform/platform-api/models";

let value: CloudRegionsResponse = {
  supportedRegions: {
    aws: [
      "<value 1>",
      "<value 2>",
      "<value 3>",
    ],
    gcp: [
      "<value 1>",
      "<value 2>",
      "<value 3>",
    ],
    azure: [
      "<value 1>",
    ],
  },
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `supportedRegions`                                                 | [models.SupportedCloudRegions](../models/supportedcloudregions.md) | :heavy_check_mark:                                                 | N/A                                                                |