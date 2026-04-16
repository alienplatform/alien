# SubmitResponseRequest

## Example Usage

```typescript
import { SubmitResponseRequest } from "@alienplatform/manager-api/models/operations";

let value: SubmitResponseRequest = {
  commandId: "<id>",
  commandResponse: {
    code: "<value>",
    message: "<value>",
    status: "error",
  },
};
```

## Fields

| Field                    | Type                     | Required                 | Description              |
| ------------------------ | ------------------------ | ------------------------ | ------------------------ |
| `commandId`              | *string*                 | :heavy_check_mark:       | Command identifier       |
| `commandResponse`        | *models.CommandResponse* | :heavy_check_mark:       | N/A                      |