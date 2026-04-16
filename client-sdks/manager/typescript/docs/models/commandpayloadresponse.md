# CommandPayloadResponse

Payload response containing params and response data from KV

## Example Usage

```typescript
import { CommandPayloadResponse } from "@alienplatform/manager-api/models";

let value: CommandPayloadResponse = {
  commandId: "<id>",
};
```

## Fields

| Field                    | Type                     | Required                 | Description              |
| ------------------------ | ------------------------ | ------------------------ | ------------------------ |
| `commandId`              | *string*                 | :heavy_check_mark:       | N/A                      |
| `params`                 | *models.BodySpec*        | :heavy_minus_sign:       | N/A                      |
| `response`               | *models.CommandResponse* | :heavy_minus_sign:       | N/A                      |