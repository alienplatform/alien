# CustomerConnectionsRequirement

## Example Usage

```typescript
import { CustomerConnectionsRequirement } from "@alienplatform/platform-api/models";

let value: CustomerConnectionsRequirement = {
  publicModelId: "<id>",
  clientApis: [
    "anthropic-messages",
  ],
  required: true,
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `publicModelId`                                                                    | *string*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `clientApis`                                                                       | [models.CustomerConnectionsClientApi](../models/customerconnectionsclientapi.md)[] | :heavy_check_mark:                                                                 | N/A                                                                                |
| `required`                                                                         | *boolean*                                                                          | :heavy_check_mark:                                                                 | N/A                                                                                |