# ProjectModels

## Example Usage

```typescript
import { ProjectModels } from "@alienplatform/platform-api/models";

let value: ProjectModels = {
  enabled: false,
  allowedProviders: [],
  requirements: [
    {
      publicModelId: "<id>",
      clientApis: [
        "anthropic-messages",
      ],
      required: false,
    },
  ],
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `enabled`                                                              | *boolean*                                                              | :heavy_check_mark:                                                     | N/A                                                                    |
| `allowedProviders`                                                     | [models.ProjectAllowedProvider](../models/projectallowedprovider.md)[] | :heavy_check_mark:                                                     | N/A                                                                    |
| `requirements`                                                         | [models.ProjectRequirement](../models/projectrequirement.md)[]         | :heavy_check_mark:                                                     | N/A                                                                    |