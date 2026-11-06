# CloudAccountComputeCleanupRequestedEvent

## Example Usage

```typescript
import { CloudAccountComputeCleanupRequestedEvent } from "@alienplatform/platform-api/models";

let value: CloudAccountComputeCleanupRequestedEvent = {
  type: "CloudAccountComputeCleanupRequested",
  cloudAccountId: "<id>",
  actor: {
    kind: "user",
    id: "<id>",
  },
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `type`                                                                                                             | *"CloudAccountComputeCleanupRequested"*                                                                            | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |
| `cloudAccountId`                                                                                                   | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |
| `actor`                                                                                                            | [models.CloudAccountComputeCleanupRequestedEventActor](../models/cloudaccountcomputecleanuprequestedeventactor.md) | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |