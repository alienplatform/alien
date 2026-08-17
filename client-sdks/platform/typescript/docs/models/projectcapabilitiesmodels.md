# ProjectCapabilitiesModels

## Example Usage

```typescript
import { ProjectCapabilitiesModels } from "@alienplatform/platform-api/models";

let value: ProjectCapabilitiesModels = {
  enabled: true,
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

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `enabled`                                                                                      | *boolean*                                                                                      | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `allowedProviders`                                                                             | [models.ProjectCapabilitiesAllowedProvider](../models/projectcapabilitiesallowedprovider.md)[] | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `requirements`                                                                                 | [models.ProjectCapabilitiesRequirement](../models/projectcapabilitiesrequirement.md)[]         | :heavy_check_mark:                                                                             | N/A                                                                                            |