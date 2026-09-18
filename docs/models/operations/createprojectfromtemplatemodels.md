# CreateProjectFromTemplateModels

## Example Usage

```typescript
import { CreateProjectFromTemplateModels } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectFromTemplateModels = {
  enabled: true,
  allowedProviders: [
    "aws-bedrock",
  ],
  requirements: [],
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `enabled`                                                                                                                    | *boolean*                                                                                                                    | :heavy_check_mark:                                                                                                           | N/A                                                                                                                          |
| `allowedProviders`                                                                                                           | [operations.CreateProjectFromTemplateAllowedProvider](../../models/operations/createprojectfromtemplateallowedprovider.md)[] | :heavy_check_mark:                                                                                                           | N/A                                                                                                                          |
| `requirements`                                                                                                               | [operations.CreateProjectFromTemplateRequirement](../../models/operations/createprojectfromtemplaterequirement.md)[]         | :heavy_check_mark:                                                                                                           | N/A                                                                                                                          |