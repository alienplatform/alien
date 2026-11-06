# CloudAccountRequestedEvent

## Example Usage

```typescript
import { CloudAccountRequestedEvent } from "@alienplatform/platform-api/models";

let value: CloudAccountRequestedEvent = {
  type: "CloudAccountRequested",
  cloudAccountId: "<id>",
  connectionId: "<id>",
  actor: {
    kind: "user",
    id: "<id>",
  },
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `type`                                                                                 | *"CloudAccountRequested"*                                                              | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `cloudAccountId`                                                                       | *string*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `connectionId`                                                                         | *string*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `actor`                                                                                | [models.CloudAccountRequestedEventActor](../models/cloudaccountrequestedeventactor.md) | :heavy_check_mark:                                                                     | N/A                                                                                    |