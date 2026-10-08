# RemoteOperatorImageIdentity

Immutable installer receipt self-reported by a running Remote Operator. This is not independent runtime image inspection.

## Example Usage

```typescript
import { RemoteOperatorImageIdentity } from "@alienplatform/platform-api/models";

let value: RemoteOperatorImageIdentity = {
  source: "configured",
  packageId: "<id>",
  packageVersion: "<value>",
  image: "https://picsum.photos/seed/lwlCU/823/2838",
  digest: "<value>",
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `source`                                                                                   | [models.RemoteOperatorImageIdentitySource](../models/remoteoperatorimageidentitysource.md) | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `packageId`                                                                                | *string*                                                                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `packageVersion`                                                                           | *string*                                                                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `image`                                                                                    | *string*                                                                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `digest`                                                                                   | *string*                                                                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |