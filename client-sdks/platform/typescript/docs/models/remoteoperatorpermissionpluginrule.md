# RemoteOperatorPermissionPluginRule

## Example Usage

```typescript
import { RemoteOperatorPermissionPluginRule } from "@alienplatform/platform-api/models";

let value: RemoteOperatorPermissionPluginRule = {
  apiGroup: "<value>",
  resource: "<value>",
  verbs: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  reason: "<value>",
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `apiGroup`         | *string*           | :heavy_check_mark: | N/A                |
| `resource`         | *string*           | :heavy_check_mark: | N/A                |
| `verbs`            | *string*[]         | :heavy_check_mark: | N/A                |
| `resourceNames`    | *string*[]         | :heavy_minus_sign: | N/A                |
| `reason`           | *string*           | :heavy_check_mark: | N/A                |