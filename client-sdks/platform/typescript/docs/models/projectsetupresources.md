# ProjectSetupResources

## Example Usage

```typescript
import { ProjectSetupResources } from "@alienplatform/platform-api/models";

let value: ProjectSetupResources = {
  tokenSecret: {
    key: "<key>",
    name: "<value>",
    prefix: "<value>",
    randomLength: 802783,
  },
  workloadReadAccess: {
    rules: [],
    serviceAccountProfile: "<value>",
  },
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `tokenSecret`                                                              | [models.ProjectTokenSecret](../models/projecttokensecret.md)               | :heavy_check_mark:                                                         | N/A                                                                        |
| `workloadReadAccess`                                                       | [models.ProjectWorkloadReadAccess](../models/projectworkloadreadaccess.md) | :heavy_check_mark:                                                         | N/A                                                                        |