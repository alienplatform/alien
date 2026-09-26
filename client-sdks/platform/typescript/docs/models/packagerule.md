# PackageRule

One Kubernetes API requirement declared by an enabled operation.

## Example Usage

```typescript
import { PackageRule } from "@alienplatform/platform-api/models";

let value: PackageRule = {
  apiGroup: "<value>",
  reason: "<value>",
  resource: "<value>",
  verbs: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `apiGroup`                                                                         | *string*                                                                           | :heavy_check_mark:                                                                 | Kubernetes API group. An empty string denotes the core API.                        |
| `reason`                                                                           | *string*                                                                           | :heavy_check_mark:                                                                 | Human-readable reason the operation requires this rule.                            |
| `resource`                                                                         | *string*                                                                           | :heavy_check_mark:                                                                 | Plural Kubernetes resource, optionally with a supported subresource.               |
| `resourceNames`                                                                    | *string*[]                                                                         | :heavy_minus_sign:                                                                 | Optional concrete resource names. Empty means all names in the installation scope. |
| `verbs`                                                                            | *string*[]                                                                         | :heavy_check_mark:                                                                 | Concrete Kubernetes API verbs required by the operation.                           |