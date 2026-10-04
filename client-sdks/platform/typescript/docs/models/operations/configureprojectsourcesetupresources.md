# ConfigureProjectSourceSetupResources

## Example Usage

```typescript
import { ConfigureProjectSourceSetupResources } from "@alienplatform/platform-api/models/operations";

let value: ConfigureProjectSourceSetupResources = {
  tokenSecret: {
    key: "<key>",
    name: "<value>",
    prefix: "<value>",
    randomLength: 976143,
  },
  workloadReadAccess: {
    rules: [],
    serviceAccountProfile: "<value>",
  },
};
```

## Fields

| Field                                                                                                                      | Type                                                                                                                       | Required                                                                                                                   | Description                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `tokenSecret`                                                                                                              | [operations.ConfigureProjectSourceTokenSecret](../../models/operations/configureprojectsourcetokensecret.md)               | :heavy_check_mark:                                                                                                         | N/A                                                                                                                        |
| `workloadReadAccess`                                                                                                       | [operations.ConfigureProjectSourceWorkloadReadAccess](../../models/operations/configureprojectsourceworkloadreadaccess.md) | :heavy_check_mark:                                                                                                         | N/A                                                                                                                        |