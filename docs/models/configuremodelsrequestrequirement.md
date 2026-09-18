# ConfigureModelsRequestRequirement

## Example Usage

```typescript
import { ConfigureModelsRequestRequirement } from "@alienplatform/platform-api/models";

let value: ConfigureModelsRequestRequirement = {
  publicModelId: "<id>",
  clientApis: [
    "openai-chat",
  ],
  required: false,
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `publicModelId`                                                                          | *string*                                                                                 | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `clientApis`                                                                             | [models.ConfigureModelsRequestClientApi](../models/configuremodelsrequestclientapi.md)[] | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `required`                                                                               | *boolean*                                                                                | :heavy_check_mark:                                                                       | N/A                                                                                      |