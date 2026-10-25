# UpdateWebhookEndpointRequest

## Example Usage

```typescript
import { UpdateWebhookEndpointRequest } from "@alienplatform/platform-api/models/operations";

let value: UpdateWebhookEndpointRequest = {
  id: "<id>",
  requestBody: {
    url: "https://brisk-mathematics.com/",
  },
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                       | *string*                                                                                                   | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `projectId`                                                                                                | *string*                                                                                                   | :heavy_minus_sign:                                                                                         | N/A                                                                                                        |
| `requestBody`                                                                                              | [operations.UpdateWebhookEndpointRequestBody](../../models/operations/updatewebhookendpointrequestbody.md) | :heavy_check_mark:                                                                                         | N/A                                                                                                        |