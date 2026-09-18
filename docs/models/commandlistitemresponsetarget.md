# CommandListItemResponseTarget

Resource the command is addressed to; null on commands created before target routing

## Example Usage

```typescript
import { CommandListItemResponseTarget } from "@alienplatform/platform-api/models";

let value: CommandListItemResponseTarget = {
  resourceId: "<id>",
  resourceType: "container",
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `resourceId`                                                                                   | *string*                                                                                       | :heavy_check_mark:                                                                             | The resource ID within the deployment's stack (e.g. a Worker/Container/Daemon id).             |
| `resourceType`                                                                                 | [models.CommandListItemResponseResourceType](../models/commandlistitemresponseresourcetype.md) | :heavy_check_mark:                                                                             | The kind of command-capable resource a command targets.                                        |