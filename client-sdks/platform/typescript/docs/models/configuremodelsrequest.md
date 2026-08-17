# ConfigureModelsRequest

## Example Usage

```typescript
import { ConfigureModelsRequest } from "@alienplatform/platform-api/models";

let value: ConfigureModelsRequest = {
  allowedProviders: [
    "gcp-vertex",
  ],
  requirements: [
    {
      publicModelId: "<id>",
      clientApis: [
        "openai-responses",
      ],
      required: false,
    },
  ],
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `allowedProviders`                                                                                   | [models.ConfigureModelsRequestAllowedProvider](../models/configuremodelsrequestallowedprovider.md)[] | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `requirements`                                                                                       | [models.ConfigureModelsRequestRequirement](../models/configuremodelsrequestrequirement.md)[]         | :heavy_check_mark:                                                                                   | N/A                                                                                                  |