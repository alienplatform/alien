# VolumeRestoreCreatedBy

Principal that requested the restore

## Example Usage

```typescript
import { VolumeRestoreCreatedBy } from "@alienplatform/platform-api/models";

let value: VolumeRestoreCreatedBy = {
  id: "<id>",
  kind: "serviceAccount",
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `email`                                                    | *string*                                                   | :heavy_minus_sign:                                         | User email when the principal is a user.                   |
| `id`                                                       | *string*                                                   | :heavy_check_mark:                                         | Stable user or service-account identifier.                 |
| `kind`                                                     | [models.VolumeRestoreKind](../models/volumerestorekind.md) | :heavy_check_mark:                                         | Type of authenticated principal that requested an event.   |
| `via`                                                      | *models.VolumeRestoreViaUnion*                             | :heavy_minus_sign:                                         | N/A                                                        |