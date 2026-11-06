# CloudAccountClosureRequestedEvent

## Example Usage

```typescript
import { CloudAccountClosureRequestedEvent } from "@alienplatform/platform-api/models";

let value: CloudAccountClosureRequestedEvent = {
  type: "CloudAccountClosureRequested",
  cloudAccountId: "<id>",
  actor: {
    kind: "serviceAccount",
    id: "<id>",
  },
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `type`                                                                                               | *"CloudAccountClosureRequested"*                                                                     | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `cloudAccountId`                                                                                     | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `actor`                                                                                              | [models.CloudAccountClosureRequestedEventActor](../models/cloudaccountclosurerequestedeventactor.md) | :heavy_check_mark:                                                                                   | N/A                                                                                                  |