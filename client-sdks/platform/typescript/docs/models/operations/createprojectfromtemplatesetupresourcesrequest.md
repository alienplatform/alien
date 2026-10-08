# CreateProjectFromTemplateSetupResourcesRequest

## Example Usage

```typescript
import { CreateProjectFromTemplateSetupResourcesRequest } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectFromTemplateSetupResourcesRequest = {
  tokenSecret: {
    key: "<key>",
    name: "<value>",
    prefix: "<value>",
    randomLength: 181606,
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

| Field                                                                                                                                          | Type                                                                                                                                           | Required                                                                                                                                       | Description                                                                                                                                    |
| ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `tokenSecret`                                                                                                                                  | [operations.CreateProjectFromTemplateTokenSecretRequest](../../models/operations/createprojectfromtemplatetokensecretrequest.md)               | :heavy_check_mark:                                                                                                                             | N/A                                                                                                                                            |
| `workloadReadAccess`                                                                                                                           | [operations.CreateProjectFromTemplateWorkloadReadAccessRequest](../../models/operations/createprojectfromtemplateworkloadreadaccessrequest.md) | :heavy_check_mark:                                                                                                                             | N/A                                                                                                                                            |