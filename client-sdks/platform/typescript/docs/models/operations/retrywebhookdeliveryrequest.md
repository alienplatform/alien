# RetryWebhookDeliveryRequest

## Example Usage

```typescript
import { RetryWebhookDeliveryRequest } from "@alienplatform/platform-api/models/operations";

let value: RetryWebhookDeliveryRequest = {
  id: "<id>",
  requestBody: {
    messageId: "<id>",
  },
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                     | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `projectId`                                                                                              | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `requestBody`                                                                                            | [operations.RetryWebhookDeliveryRequestBody](../../models/operations/retrywebhookdeliveryrequestbody.md) | :heavy_check_mark:                                                                                       | N/A                                                                                                      |