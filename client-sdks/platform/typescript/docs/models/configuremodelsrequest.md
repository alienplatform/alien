# ConfigureModelsRequest

## Example Usage

```typescript
import { ConfigureModelsRequest } from "@alienplatform/platform-api/models";

let value: ConfigureModelsRequest = {
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

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `allowedProviders`                                                                                   | [models.ConfigureModelsRequestAllowedProvider](../models/configuremodelsrequestallowedprovider.md)[] | :heavy_minus_sign:                                                                                   | N/A                                                                                                  |
| `requirements`                                                                                       | [models.ConfigureModelsRequestRequirement](../models/configuremodelsrequestrequirement.md)[]         | :heavy_check_mark:                                                                                   | N/A                                                                                                  |