# ObservedRemoteOperatorImageIdentity

## Example Usage

```typescript
import { ObservedRemoteOperatorImageIdentity } from "@alienplatform/platform-api/models";

let value: ObservedRemoteOperatorImageIdentity = {
  source: "configured",
  packageId: "<id>",
  packageVersion: null,
  image: "https://loremflickr.com/3580/3651?lock=4791108270352785",
  digest: "<value>",
  observedAt: new Date("2025-05-23T13:12:09.440Z"),
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `source`                                                                                                   | [models.ObservedRemoteOperatorImageIdentitySource](../models/observedremoteoperatorimageidentitysource.md) | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `packageId`                                                                                                | *string*                                                                                                   | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `packageVersion`                                                                                           | *string*                                                                                                   | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `image`                                                                                                    | *string*                                                                                                   | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `digest`                                                                                                   | *string*                                                                                                   | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `observedAt`                                                                                               | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)              | :heavy_check_mark:                                                                                         | N/A                                                                                                        |