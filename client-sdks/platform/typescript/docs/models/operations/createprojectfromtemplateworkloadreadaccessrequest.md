# CreateProjectFromTemplateWorkloadReadAccessRequest

## Example Usage

```typescript
import { CreateProjectFromTemplateWorkloadReadAccessRequest } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectFromTemplateWorkloadReadAccessRequest = {
  rules: [
    {
      apiGroup: "<value>",
      resources: [
        "<value 1>",
      ],
    },
  ],
  serviceAccountProfile: "<value>",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `rules`                                                                                                              | [operations.CreateProjectFromTemplateRuleRequest](../../models/operations/createprojectfromtemplaterulerequest.md)[] | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `serviceAccountProfile`                                                                                              | *string*                                                                                                             | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
