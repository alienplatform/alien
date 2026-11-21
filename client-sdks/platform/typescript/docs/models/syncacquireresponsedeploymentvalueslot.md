# SyncAcquireResponseDeploymentValueSlot

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentValueSlot } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentValueSlot = {
  slot: "<value>",
  kind: "plain",
  digest: "<value>",
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `slot`                                                                                     | *string*                                                                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `kind`                                                                                     | [models.SyncAcquireResponseDeploymentKind](../models/syncacquireresponsedeploymentkind.md) | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `digest`                                                                                   | *string*                                                                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `envName`                                                                                  | *string*                                                                                   | :heavy_minus_sign:                                                                         | N/A                                                                                        |
| `targets`                                                                                  | *string*[]                                                                                 | :heavy_minus_sign:                                                                         | N/A                                                                                        |