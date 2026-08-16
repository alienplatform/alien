# ProjectCapabilitiesRequirement

## Example Usage

```typescript
import { ProjectCapabilitiesRequirement } from "@alienplatform/platform-api/models";

let value: ProjectCapabilitiesRequirement = {
  publicModelId: "<id>",
  clientApis: [
    "openai-chat",
  ],
  required: true,
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `publicModelId`                                                                    | *string*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `clientApis`                                                                       | [models.ProjectCapabilitiesClientApi](../models/projectcapabilitiesclientapi.md)[] | :heavy_check_mark:                                                                 | N/A                                                                                |
| `required`                                                                         | *boolean*                                                                          | :heavy_check_mark:                                                                 | N/A                                                                                |