# RuntimePurposeReceiveCommands

Receive commands for one app-owned target.

## Example Usage

```typescript
import { RuntimePurposeReceiveCommands } from "@alienplatform/manager-api/models";

let value: RuntimePurposeReceiveCommands = {
  target: {
    resourceId: "<id>",
    resourceType: "daemon",
  },
  type: "receiveCommands",
};
```

## Fields

| Field                                                       | Type                                                        | Required                                                    | Description                                                 |
| ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- |
| `target`                                                    | [models.CommandTarget](../models/commandtarget.md)          | :heavy_check_mark:                                          | Identifies the specific resource a command is addressed to. |
| `type`                                                      | *"receiveCommands"*                                         | :heavy_check_mark:                                          | N/A                                                         |
