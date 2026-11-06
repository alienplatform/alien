# CloudAccountSetupCleanupRequestedEvent

## Example Usage

```typescript
import { CloudAccountSetupCleanupRequestedEvent } from "@alienplatform/platform-api/models";

let value: CloudAccountSetupCleanupRequestedEvent = {
  type: "CloudAccountSetupCleanupRequested",
  cloudAccountId: "<id>",
  actor: {
    kind: "serviceAccount",
    id: "<id>",
  },
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `type`                                                                                                         | *"CloudAccountSetupCleanupRequested"*                                                                          | :heavy_check_mark:                                                                                             | N/A                                                                                                            |
| `cloudAccountId`                                                                                               | *string*                                                                                                       | :heavy_check_mark:                                                                                             | N/A                                                                                                            |
| `actor`                                                                                                        | [models.CloudAccountSetupCleanupRequestedEventActor](../models/cloudaccountsetupcleanuprequestedeventactor.md) | :heavy_check_mark:                                                                                             | N/A                                                                                                            |