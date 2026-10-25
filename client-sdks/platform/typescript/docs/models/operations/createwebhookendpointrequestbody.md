# CreateWebhookEndpointRequestBody

## Example Usage

```typescript
import { CreateWebhookEndpointRequestBody } from "@alienplatform/platform-api/models/operations";

let value: CreateWebhookEndpointRequestBody = {
  url: "https://accomplished-seagull.net/",
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `url`                                                                                                      | *string*                                                                                                   | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `description`                                                                                              | *string*                                                                                                   | :heavy_minus_sign:                                                                                         | N/A                                                                                                        |
| `filterTypes`                                                                                              | [operations.CreateWebhookEndpointFilterType](../../models/operations/createwebhookendpointfiltertype.md)[] | :heavy_minus_sign:                                                                                         | N/A                                                                                                        |
| `disabled`                                                                                                 | *boolean*                                                                                                  | :heavy_minus_sign:                                                                                         | N/A                                                                                                        |
| `projectIds`                                                                                               | *string*[]                                                                                                 | :heavy_minus_sign:                                                                                         | N/A                                                                                                        |