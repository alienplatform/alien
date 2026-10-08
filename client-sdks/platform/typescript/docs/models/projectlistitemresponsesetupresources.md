# ProjectListItemResponseSetupResources

## Example Usage

```typescript
import { ProjectListItemResponseSetupResources } from "@alienplatform/platform-api/models";

let value: ProjectListItemResponseSetupResources = {
  tokenSecret: {
    key: "<key>",
    name: "<value>",
    prefix: "<value>",
    randomLength: 102727,
  },
  workloadReadAccess: {
    rules: [],
    serviceAccountProfile: "<value>",
  },
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `tokenSecret`                                                                                              | [models.ProjectListItemResponseTokenSecret](../models/projectlistitemresponsetokensecret.md)               | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `workloadReadAccess`                                                                                       | [models.ProjectListItemResponseWorkloadReadAccess](../models/projectlistitemresponseworkloadreadaccess.md) | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
