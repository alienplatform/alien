# CreateProjectSetupResourcesRequest

## Example Usage

```typescript
import { CreateProjectSetupResourcesRequest } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectSetupResourcesRequest = {
  tokenSecret: {
    key: "<key>",
    name: "<value>",
    prefix: "<value>",
    randomLength: 380161,
  },
  workloadReadAccess: {
    rules: [
      {
        apiGroup: "<value>",
        resources: [
          "<value 1>",
          "<value 2>",
        ],
      },
    ],
    serviceAccountProfile: "<value>",
  },
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `tokenSecret`                                                                                                          | [operations.CreateProjectTokenSecretRequest](../../models/operations/createprojecttokensecretrequest.md)               | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `workloadReadAccess`                                                                                                   | [operations.CreateProjectWorkloadReadAccessRequest](../../models/operations/createprojectworkloadreadaccessrequest.md) | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |