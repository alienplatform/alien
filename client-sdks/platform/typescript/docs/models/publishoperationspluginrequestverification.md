# PublishOperationsPluginRequestVerification

## Example Usage

```typescript
import { PublishOperationsPluginRequestVerification } from "@alienplatform/platform-api/models";

let value: PublishOperationsPluginRequestVerification = {
  changes: "<value>",
  pollOperation: "<value>",
  successField: "<value>",
  successValue: "<value>",
  timeoutSeconds: 303273,
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `changes`                                                                                      | *string*                                                                                       | :heavy_check_mark:                                                                             | Human-readable: what this operation changes.                                                   |
| `pollOperation`                                                                                | *string*                                                                                       | :heavy_check_mark:                                                                             | A read-only operation in the same plugin to poll for success.                                  |
| `pollParamsFromResult`                                                                         | Record<string, *string*>                                                                       | :heavy_minus_sign:                                                                             | Poll operation param name -> field name in the write operation's own result.                   |
| `successField`                                                                                 | *string*                                                                                       | :heavy_check_mark:                                                                             | Dotted field path in the poll result that signals success.                                     |
| `successValue`                                                                                 | *string*                                                                                       | :heavy_check_mark:                                                                             | The value at successField that means verified.                                                 |
| `retry`                                                                                        | [models.PublishOperationsPluginRequestRetry](../models/publishoperationspluginrequestretry.md) | :heavy_minus_sign:                                                                             | N/A                                                                                            |
| `timeoutSeconds`                                                                               | *number*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |