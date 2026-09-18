# ProjectRequirement

## Example Usage

```typescript
import { ProjectRequirement } from "@alienplatform/platform-api/models";

let value: ProjectRequirement = {
  publicModelId: "<id>",
  clientApis: [
    "openai-chat",
  ],
  required: true,
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `publicModelId`                                            | *string*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `clientApis`                                               | [models.ProjectClientApi](../models/projectclientapi.md)[] | :heavy_check_mark:                                         | N/A                                                        |
| `required`                                                 | *boolean*                                                  | :heavy_check_mark:                                         | N/A                                                        |