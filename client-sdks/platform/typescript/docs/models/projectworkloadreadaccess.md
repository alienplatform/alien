# ProjectWorkloadReadAccess

## Example Usage

```typescript
import { ProjectWorkloadReadAccess } from "@alienplatform/platform-api/models";

let value: ProjectWorkloadReadAccess = {
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

| Field                                            | Type                                             | Required                                         | Description                                      |
| ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ |
| `rules`                                          | [models.ProjectRule](../models/projectrule.md)[] | :heavy_check_mark:                               | N/A                                              |
| `serviceAccountProfile`                          | *string*                                         | :heavy_check_mark:                               | N/A                                              |
