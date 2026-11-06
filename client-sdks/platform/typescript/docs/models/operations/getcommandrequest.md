# GetCommandRequest

## Example Usage

```typescript
import { GetCommandRequest } from "@alienplatform/platform-api/models/operations";

let value: GetCommandRequest = {
  id: "cmd_2sxjXxvOYct7IohT3ukliAzf7Nzb",
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            | Example                                                                                |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `id`                                                                                   | *string*                                                                               | :heavy_check_mark:                                                                     | Unique identifier for the command.                                                     | cmd_2sxjXxvOYct7IohT3ukliAzf7Nzb                                                       |
| `confirmSensitiveOutput`                                                               | [operations.ConfirmSensitiveOutput](../../models/operations/confirmsensitiveoutput.md) | :heavy_minus_sign:                                                                     | Explicitly confirm access to an operation result marked sensitive.                     |                                                                                        |