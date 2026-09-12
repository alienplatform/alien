# CreateProjectFromTemplateModels

## Example Usage

```typescript
import { CreateProjectFromTemplateModels } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectFromTemplateModels = {
  enabled: true,
  allowedProviders: [],
  requirements: [
    {
      publicModelId: "<id>",
      clientApis: [],
      required: true,
    },
  ],
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `enabled`                                                                                                                    | *true*                                                                                                                       | :heavy_check_mark:                                                                                                           | N/A                                                                                                                          |
| `allowedProviders`                                                                                                           | [operations.CreateProjectFromTemplateAllowedProvider](../../models/operations/createprojectfromtemplateallowedprovider.md)[] | :heavy_check_mark:                                                                                                           | N/A                                                                                                                          |
| `requirements`                                                                                                               | [operations.CreateProjectFromTemplateRequirement](../../models/operations/createprojectfromtemplaterequirement.md)[]         | :heavy_check_mark:                                                                                                           | N/A                                                                                                                          |