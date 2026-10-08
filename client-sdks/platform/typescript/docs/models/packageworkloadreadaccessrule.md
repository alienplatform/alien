# PackageWorkloadReadAccessRule

Resource family the chart grants get/list/watch over.

## Example Usage

```typescript
import { PackageWorkloadReadAccessRule } from "@alienplatform/platform-api/models";

let value: PackageWorkloadReadAccessRule = {
  apiGroup: "<value>",
  resources: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
};
```

## Fields

| Field                                                         | Type                                                          | Required                                                      | Description                                                   |
| ------------------------------------------------------------- | ------------------------------------------------------------- | ------------------------------------------------------------- | ------------------------------------------------------------- |
| `apiGroup`                                                    | *string*                                                      | :heavy_check_mark:                                            | Empty string for the core Kubernetes API group.               |
| `resources`                                                   | *string*[]                                                    | :heavy_check_mark:                                            | Kubernetes plural resource names; wildcards are not accepted. |
