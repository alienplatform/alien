# HealthResponse

## Example Usage

```typescript
import { HealthResponse } from "@alienplatform/manager-api/models";

let value: HealthResponse = {
  operationResultContract: false,
  status: "<value>",
};
```

## Fields

| Field                                                                                 | Type                                                                                  | Required                                                                              | Description                                                                           |
| ------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| `operationResultContract`                                                             | *boolean*                                                                             | :heavy_check_mark:                                                                    | True when operation result contracts are persisted before commands<br/>become executable. |
| `status`                                                                              | *string*                                                                              | :heavy_check_mark:                                                                    | N/A                                                                                   |