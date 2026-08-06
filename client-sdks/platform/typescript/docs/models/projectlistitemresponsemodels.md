# ProjectListItemResponseModels

## Example Usage

```typescript
import { ProjectListItemResponseModels } from "@alienplatform/platform-api/models";

let value: ProjectListItemResponseModels = {
  enabled: false,
  allowedProviders: [],
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

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `enabled`                                                                                              | *boolean*                                                                                              | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `allowedProviders`                                                                                     | [models.ProjectListItemResponseAllowedProvider](../models/projectlistitemresponseallowedprovider.md)[] | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `requirements`                                                                                         | [models.ProjectListItemResponseRequirement](../models/projectlistitemresponserequirement.md)[]         | :heavy_check_mark:                                                                                     | N/A                                                                                                    |