# CloudAccountAssignedEvent

## Example Usage

```typescript
import { CloudAccountAssignedEvent } from "@alienplatform/platform-api/models";

let value: CloudAccountAssignedEvent = {
  type: "CloudAccountAssigned",
  cloudAccountId: "<id>",
  deploymentGroupId: "<id>",
  actor: {
    kind: "serviceAccount",
    id: "<id>",
  },
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `type`                                                                               | *"CloudAccountAssigned"*                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `cloudAccountId`                                                                     | *string*                                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `deploymentGroupId`                                                                  | *string*                                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `actor`                                                                              | [models.CloudAccountAssignedEventActor](../models/cloudaccountassignedeventactor.md) | :heavy_check_mark:                                                                   | N/A                                                                                  |