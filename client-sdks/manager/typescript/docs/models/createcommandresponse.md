# CreateCommandResponse

Response to command creation

## Example Usage

```typescript
import { CreateCommandResponse } from "@alienplatform/manager-api/models";

let value: CreateCommandResponse = {
  commandId: "<id>",
  inlineAllowedUpTo: 344413,
  next: "<value>",
  state: "DISPATCHED",
};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `commandId`                                        | *string*                                           | :heavy_check_mark:                                 | Unique command identifier                          |
| `inlineAllowedUpTo`                                | *number*                                           | :heavy_check_mark:                                 | Maximum inline body size allowed                   |
| `next`                                             | *string*                                           | :heavy_check_mark:                                 | Next action for client: "upload" \| "poll"         |
| `state`                                            | [models.CommandState](../models/commandstate.md)   | :heavy_check_mark:                                 | Command states in the Commands protocol lifecycle  |
| `storageUpload`                                    | [models.StorageUpload](../models/storageupload.md) | :heavy_minus_sign:                                 | N/A                                                |