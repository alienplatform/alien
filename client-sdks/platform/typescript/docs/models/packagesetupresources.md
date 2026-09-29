# PackageSetupResources

Kubernetes resources created by the product chart before the runtime starts.

## Example Usage

```typescript
import { PackageSetupResources } from "@alienplatform/platform-api/models";

let value: PackageSetupResources = {
  tokenSecret: {
    key: "<key>",
    name: "<value>",
    prefix: "<value>",
    randomLength: 902280,
  },
  workloadReadAccess: {
    rules: [
      {
        apiGroup: "<value>",
        resources: [],
      },
    ],
    serviceAccountProfile: "<value>",
  },
};
```

## Fields

| Field                                                                             | Type                                                                              | Required                                                                          | Description                                                                       |
| --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| `tokenSecret`                                                                     | [models.PackageTokenSecret](../models/packagetokensecret.md)                      | :heavy_check_mark:                                                                | Non-secret configuration for a chart-generated enrollment token.                  |
| `workloadReadAccess`                                                              | [models.PackageWorkloadReadAccess](../models/packageworkloadreadaccess.md)        | :heavy_check_mark:                                                                | Cluster reads granted to a workload identity, separate from Operator permissions. |