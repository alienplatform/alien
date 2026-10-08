# ConfigureProjectSourceWorkloadReadAccess

## Example Usage

```typescript
import { ConfigureProjectSourceWorkloadReadAccess } from "@alienplatform/platform-api/models/operations";

let value: ConfigureProjectSourceWorkloadReadAccess = {
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

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `rules`                                                                                          | [operations.ConfigureProjectSourceRule](../../models/operations/configureprojectsourcerule.md)[] | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `serviceAccountProfile`                                                                          | *string*                                                                                         | :heavy_check_mark:                                                                               | N/A                                                                                              |
