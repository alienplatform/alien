# DebugSessionPublicErrorCause

## Example Usage

```typescript
import { DebugSessionPublicErrorCause } from "@alienplatform/platform-api/models";

let value: DebugSessionPublicErrorCause = {
  code: "<value>",
  internal: false,
  message: "<value>",
  retryable: false,
};
```

## Fields

| Field                                   | Type                                    | Required                                | Description                             |
| --------------------------------------- | --------------------------------------- | --------------------------------------- | --------------------------------------- |
| `code`                                  | *string*                                | :heavy_check_mark:                      | N/A                                     |
| `context`                               | *models.DebugSessionPublicErrorContext* | :heavy_minus_sign:                      | N/A                                     |
| `hint`                                  | *string*                                | :heavy_minus_sign:                      | N/A                                     |
| `httpStatusCode`                        | *number*                                | :heavy_minus_sign:                      | N/A                                     |
| `internal`                              | *boolean*                               | :heavy_check_mark:                      | N/A                                     |
| `message`                               | *string*                                | :heavy_check_mark:                      | N/A                                     |
| `retryable`                             | *boolean*                               | :heavy_check_mark:                      | N/A                                     |
