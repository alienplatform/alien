# CommandTarget

Resource the command is addressed to; null on commands created before target routing

## Example Usage

```typescript
import { CommandTarget } from "@alienplatform/platform-api/models";

let value: CommandTarget = {
  resourceId: "<id>",
  resourceType: "worker",
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `resourceId`                                                                       | *string*                                                                           | :heavy_check_mark:                                                                 | The resource ID within the deployment's stack (e.g. a Worker/Container/Daemon id). |
| `resourceType`                                                                     | [models.CommandResourceType](../models/commandresourcetype.md)                     | :heavy_check_mark:                                                                 | The kind of command-capable resource a command targets.                            |