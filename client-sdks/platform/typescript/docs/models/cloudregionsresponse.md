# CloudRegionsResponse

## Example Usage

```typescript
import { CloudRegionsResponse } from "@alienplatform/platform-api/models";

let value: CloudRegionsResponse = {
  supportedRegions: {
    aws: [],
    gcp: [
      "<value 1>",
    ],
    azure: [
      "<value 1>",
      "<value 2>",
    ],
  },
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `supportedRegions`                                                 | [models.SupportedCloudRegions](../models/supportedcloudregions.md) | :heavy_check_mark:                                                 | N/A                                                                |