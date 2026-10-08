# CreateCommandResponseTarget

Resource the command is addressed to

## Example Usage

```typescript
import { CreateCommandResponseTarget } from "@alienplatform/platform-api/models";

let value: CreateCommandResponseTarget = {
  resourceId: "<id>",
  resourceType: "daemon",
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `resourceId`                                                                               | *string*                                                                                   | :heavy_check_mark:                                                                         | The resource ID within the deployment's stack (e.g. a Worker/Container/Daemon id).         |
| `resourceType`                                                                             | [models.CreateCommandResponseResourceType](../models/createcommandresponseresourcetype.md) | :heavy_check_mark:                                                                         | The kind of command-capable resource a command targets.                                    |