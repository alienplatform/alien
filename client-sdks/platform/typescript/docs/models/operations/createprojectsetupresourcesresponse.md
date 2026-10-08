# CreateProjectSetupResourcesResponse

## Example Usage

```typescript
import { CreateProjectSetupResourcesResponse } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectSetupResourcesResponse = {
  tokenSecret: {
    key: "<key>",
    name: "<value>",
    prefix: "<value>",
    randomLength: 403761,
  },
  workloadReadAccess: {
    rules: [
      {
        apiGroup: "<value>",
        resources: [
          "<value 1>",
        ],
      },
    ],
    serviceAccountProfile: "<value>",
  },
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `tokenSecret`                                                                                                            | [operations.CreateProjectTokenSecretResponse](../../models/operations/createprojecttokensecretresponse.md)               | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `workloadReadAccess`                                                                                                     | [operations.CreateProjectWorkloadReadAccessResponse](../../models/operations/createprojectworkloadreadaccessresponse.md) | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
