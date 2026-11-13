# RuntimePurposeOperationRelay

Relay commands to one app worker, without dispatch or administrative rights.

## Example Usage

```typescript
import { RuntimePurposeOperationRelay } from "@alienplatform/manager-api/models";

let value: RuntimePurposeOperationRelay = {
  target: {
    resourceId: "<id>",
    resourceType: "daemon",
  },
  type: "operationRelay",
};
```

## Fields

| Field                                                       | Type                                                        | Required                                                    | Description                                                 |
| ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- |
| `target`                                                    | [models.CommandTarget](../models/commandtarget.md)          | :heavy_check_mark:                                          | Identifies the specific resource a command is addressed to. |
| `type`                                                      | *"operationRelay"*                                          | :heavy_check_mark:                                          | N/A                                                         |
