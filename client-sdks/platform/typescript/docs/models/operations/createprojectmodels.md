# CreateProjectModels

## Example Usage

```typescript
import { CreateProjectModels } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectModels = {
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

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `enabled`                                                                                            | *true*                                                                                               | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `allowedProviders`                                                                                   | [operations.CreateProjectAllowedProvider](../../models/operations/createprojectallowedprovider.md)[] | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `requirements`                                                                                       | [operations.CreateProjectRequirement](../../models/operations/createprojectrequirement.md)[]         | :heavy_check_mark:                                                                                   | N/A                                                                                                  |