# ResolveCommandResultConnectionResponse

Exact command result connection.

## Example Usage

```typescript
import { ResolveCommandResultConnectionResponse } from "@alienplatform/platform-api/models/operations";

let value: ResolveCommandResultConnectionResponse = {
  managerUrl: "https://cluttered-cross-contamination.name/",
  token: "<value>",
  commandId: "cmd_2sxjXxvOYct7IohT3ukliAzf7Nzb",
};
```

## Fields

| Field                              | Type                               | Required                           | Description                        | Example                            |
| ---------------------------------- | ---------------------------------- | ---------------------------------- | ---------------------------------- | ---------------------------------- |
| `managerUrl`                       | *string*                           | :heavy_check_mark:                 | N/A                                |                                    |
| `token`                            | *string*                           | :heavy_check_mark:                 | N/A                                |                                    |
| `commandId`                        | *string*                           | :heavy_check_mark:                 | Unique identifier for the command. | cmd_2sxjXxvOYct7IohT3ukliAzf7Nzb   |
| `operationResultContract`          | *any*                              | :heavy_minus_sign:                 | N/A                                |                                    |