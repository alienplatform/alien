# CreateProjectFromTemplateSetupResourcesResponse

## Example Usage

```typescript
import { CreateProjectFromTemplateSetupResourcesResponse } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectFromTemplateSetupResourcesResponse = {
  tokenSecret: {
    key: "<key>",
    name: "<value>",
    prefix: "<value>",
    randomLength: 749181,
  },
  workloadReadAccess: {
    rules: [],
    serviceAccountProfile: "<value>",
  },
};
```

## Fields

| Field                                                                                                                                            | Type                                                                                                                                             | Required                                                                                                                                         | Description                                                                                                                                      |
| ------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| `tokenSecret`                                                                                                                                    | [operations.CreateProjectFromTemplateTokenSecretResponse](../../models/operations/createprojectfromtemplatetokensecretresponse.md)               | :heavy_check_mark:                                                                                                                               | N/A                                                                                                                                              |
| `workloadReadAccess`                                                                                                                             | [operations.CreateProjectFromTemplateWorkloadReadAccessResponse](../../models/operations/createprojectfromtemplateworkloadreadaccessresponse.md) | :heavy_check_mark:                                                                                                                               | N/A                                                                                                                                              |