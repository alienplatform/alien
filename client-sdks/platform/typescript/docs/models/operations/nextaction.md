# NextAction

## Example Usage

```typescript
import { NextAction } from "@alienplatform/platform-api/models/operations";

let value: NextAction = {
  kind: "reconnect",
  target: "access",
  deploymentId: "<id>",
  message: "<value>",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `kind`                                                                 | [operations.NextActionKind](../../models/operations/nextactionkind.md) | :heavy_check_mark:                                                     | N/A                                                                    |
| `target`                                                               | [operations.Target](../../models/operations/target.md)                 | :heavy_check_mark:                                                     | N/A                                                                    |
| `deploymentId`                                                         | *string*                                                               | :heavy_check_mark:                                                     | N/A                                                                    |
| `message`                                                              | *string*                                                               | :heavy_check_mark:                                                     | N/A                                                                    |