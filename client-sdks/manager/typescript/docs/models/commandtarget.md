# CommandTarget

Identifies the specific resource a command is addressed to.

## Example Usage

```typescript
import { CommandTarget } from "@alienplatform/manager-api/models";

let value: CommandTarget = {
  resourceId: "<id>",
  resourceType: "worker",
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `resourceId`                                                                       | *string*                                                                           | :heavy_check_mark:                                                                 | The resource ID within the deployment's stack (e.g. a Worker/Container/Daemon id). |
| `resourceType`                                                                     | [models.CommandTargetType](../models/commandtargettype.md)                         | :heavy_check_mark:                                                                 | The kind of command-capable resource a command targets.                            |