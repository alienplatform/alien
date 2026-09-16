# CreateCommandResponse

Response to command creation

## Example Usage

```typescript
import { CreateCommandResponse } from "@alienplatform/manager-api/models";

let value: CreateCommandResponse = {
  commandId: "<id>",
  created: true,
  inlineAllowedUpTo: 375670,
  next: "<value>",
  state: "SUCCEEDED",
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `commandId`                                                                                                                        | *string*                                                                                                                           | :heavy_check_mark:                                                                                                                 | Unique command identifier                                                                                                          |
| `created`                                                                                                                          | *boolean*                                                                                                                          | :heavy_check_mark:                                                                                                                 | Whether this request created the returned command. False means an<br/>idempotent replay returned a command created by another request. |
| `inlineAllowedUpTo`                                                                                                                | *number*                                                                                                                           | :heavy_check_mark:                                                                                                                 | Maximum inline body size allowed                                                                                                   |
| `next`                                                                                                                             | *string*                                                                                                                           | :heavy_check_mark:                                                                                                                 | Next action for client: "upload" \| "poll"                                                                                         |
| `state`                                                                                                                            | [models.CommandState](../models/commandstate.md)                                                                                   | :heavy_check_mark:                                                                                                                 | Command states in the Commands protocol lifecycle                                                                                  |
| `storageUpload`                                                                                                                    | [models.StorageUpload](../models/storageupload.md)                                                                                 | :heavy_minus_sign:                                                                                                                 | N/A                                                                                                                                |