# CreateProjectWorkloadReadAccessRequest

## Example Usage

```typescript
import { CreateProjectWorkloadReadAccessRequest } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectWorkloadReadAccessRequest = {
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
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `rules`                                                                                      | [operations.CreateProjectRuleRequest](../../models/operations/createprojectrulerequest.md)[] | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `serviceAccountProfile`                                                                      | *string*                                                                                     | :heavy_check_mark:                                                                           | N/A                                                                                          |
