# PackageWorkloadReadAccess

Cluster reads granted to a workload identity, separate from Operator permissions.

## Example Usage

```typescript
import { PackageWorkloadReadAccess } from "@alienplatform/platform-api/models";

let value: PackageWorkloadReadAccess = {
  rules: [
    {
      apiGroup: "<value>",
      resources: [],
    },
  ],
  serviceAccountProfile: "<value>",
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `rules`                                                                              | [models.PackageWorkloadReadAccessRule](../models/packageworkloadreadaccessrule.md)[] | :heavy_check_mark:                                                                   | Exact API groups and resources. Verbs are fixed to get/list/watch.                   |
| `serviceAccountProfile`                                                              | *string*                                                                             | :heavy_check_mark:                                                                   | Permission profile used by the workload Container and generated ServiceAccount.      |