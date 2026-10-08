# CreateProjectFromTemplateRequirement

## Example Usage

```typescript
import { CreateProjectFromTemplateRequirement } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectFromTemplateRequirement = {
  publicModelId: "<id>",
  clientApis: [
    "openai-responses",
  ],
  required: false,
};
```

## Fields

| Field                                                                                                            | Type                                                                                                             | Required                                                                                                         | Description                                                                                                      |
| ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `publicModelId`                                                                                                  | *string*                                                                                                         | :heavy_check_mark:                                                                                               | N/A                                                                                                              |
| `clientApis`                                                                                                     | [operations.CreateProjectFromTemplateClientApi](../../models/operations/createprojectfromtemplateclientapi.md)[] | :heavy_check_mark:                                                                                               | N/A                                                                                                              |
| `required`                                                                                                       | *boolean*                                                                                                        | :heavy_check_mark:                                                                                               | N/A                                                                                                              |