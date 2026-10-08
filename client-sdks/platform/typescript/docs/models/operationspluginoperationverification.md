# OperationsPluginOperationVerification

## Example Usage

```typescript
import { OperationsPluginOperationVerification } from "@alienplatform/platform-api/models";

let value: OperationsPluginOperationVerification = {
  changes: "<value>",
  pollOperation: "<value>",
  pollParamsFromResult: {
    "key": "<value>",
    "key1": "<value>",
  },
  successField: "<value>",
  successValue: "<value>",
  timeoutSeconds: 36932,
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `changes`                                                                            | *string*                                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `pollOperation`                                                                      | *string*                                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `pollParamsFromResult`                                                               | Record<string, *string*>                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `successField`                                                                       | *string*                                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `successValue`                                                                       | *string*                                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `retry`                                                                              | [models.OperationsPluginOperationRetry](../models/operationspluginoperationretry.md) | :heavy_minus_sign:                                                                   | N/A                                                                                  |
| `timeoutSeconds`                                                                     | *number*                                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |