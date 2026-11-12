# DeleteValueRequest

## Example Usage

```typescript
import { DeleteValueRequest } from "@alienplatform/manager-api/models/operations";

let value: DeleteValueRequest = {
  scope: "<value>",
  slot: "<value>",
};
```

## Fields

| Field                                                 | Type                                                  | Required                                              | Description                                           |
| ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- |
| `scope`                                               | *string*                                              | :heavy_check_mark:                                    | dep:<deploymentId>                                    |
| `slot`                                                | *string*                                              | :heavy_check_mark:                                    | env:<NAME>, input:<id> or cenv.<containerName>.<NAME> |
