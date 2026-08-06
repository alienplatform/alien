# ConfigureCustomerModelsRequest

## Example Usage

```typescript
import { ConfigureCustomerModelsRequest } from "@alienplatform/platform-api/models";

let value: ConfigureCustomerModelsRequest = {
  allowedProviders: [
    "azure-foundry",
  ],
  requirements: [
    {
      publicModelId: "<id>",
      clientApis: [],
      required: false,
    },
  ],
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `allowedProviders`                                                                                                   | [models.ConfigureCustomerModelsRequestAllowedProvider](../models/configurecustomermodelsrequestallowedprovider.md)[] | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `requirements`                                                                                                       | [models.ConfigureCustomerModelsRequestRequirement](../models/configurecustomermodelsrequestrequirement.md)[]         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |