# CommandResponseError

Command failed with an error

## Example Usage

```typescript
import { CommandResponseError } from "@alienplatform/manager-api/models";

let value: CommandResponseError = {
  code: "<value>",
  message: "<value>",
  status: "error",
};
```

## Fields

| Field                       | Type                        | Required                    | Description                 |
| --------------------------- | --------------------------- | --------------------------- | --------------------------- |
| `code`                      | *string*                    | :heavy_check_mark:          | Error code                  |
| `details`                   | *string*                    | :heavy_minus_sign:          | Optional additional details |
| `message`                   | *string*                    | :heavy_check_mark:          | Error message               |
| `status`                    | *"error"*                   | :heavy_check_mark:          | N/A                         |