# ApproveAccessRequestRequestBody

## Example Usage

```typescript
import { ApproveAccessRequestRequestBody } from "@alienplatform/platform-api/models/operations";

let value: ApproveAccessRequestRequestBody = {
  method: "slack",
  actorId: "<id>",
};
```

## Fields

| Field                 | Type                  | Required              | Description           | Example               |
| --------------------- | --------------------- | --------------------- | --------------------- | --------------------- |
| `method`              | *string*              | :heavy_check_mark:    | N/A                   | slack                 |
| `actorId`             | *string*              | :heavy_check_mark:    | N/A                   |                       |
| `source`              | Record<string, *any*> | :heavy_minus_sign:    | N/A                   |                       |
| `approvedForMinutes`  | *number*              | :heavy_minus_sign:    | N/A                   |                       |