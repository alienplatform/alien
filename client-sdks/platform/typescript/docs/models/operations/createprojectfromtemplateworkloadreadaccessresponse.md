# CreateProjectFromTemplateWorkloadReadAccessResponse

## Example Usage

```typescript
import { CreateProjectFromTemplateWorkloadReadAccessResponse } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectFromTemplateWorkloadReadAccessResponse = {
  rules: [
    {
      apiGroup: "<value>",
      resources: [
        "<value 1>",
        "<value 2>",
        "<value 3>",
      ],
    },
  ],
  serviceAccountProfile: "<value>",
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `rules`                                                                                                                | [operations.CreateProjectFromTemplateRuleResponse](../../models/operations/createprojectfromtemplateruleresponse.md)[] | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `serviceAccountProfile`                                                                                                | *string*                                                                                                               | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |