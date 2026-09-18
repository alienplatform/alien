# CommandReceiverBootstrapTarget

Resolved target identity; present only for receiver bootstrap

## Example Usage

```typescript
import { CommandReceiverBootstrapTarget } from "@alienplatform/platform-api/models";

let value: CommandReceiverBootstrapTarget = {
  resourceId: "<id>",
  resourceType: "daemon",
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `resourceId`                                                                                                 | *string*                                                                                                     | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `resourceType`                                                                                               | [models.CommandReceiverBootstrapTargetResourceType](../models/commandreceiverbootstraptargetresourcetype.md) | :heavy_check_mark:                                                                                           | N/A                                                                                                          |