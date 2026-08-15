# InvokeOperationRequest

## Example Usage

```typescript
import { InvokeOperationRequest } from "@alienplatform/platform-api/models";

let value: InvokeOperationRequest = {
  deploymentId: "<id>",
  plugin: "<value>",
  operation: "<value>",
};
```

## Fields

| Field                                    | Type                                     | Required                                 | Description                              |
| ---------------------------------------- | ---------------------------------------- | ---------------------------------------- | ---------------------------------------- |
| `deploymentId`                           | *string*                                 | :heavy_check_mark:                       | Deployment to run the operation against. |
| `plugin`                                 | *string*                                 | :heavy_check_mark:                       | Plugin name.                             |
| `operation`                              | *string*                                 | :heavy_check_mark:                       | Operation name exposed by the plugin.    |
| `params`                                 | *any*                                    | :heavy_minus_sign:                       | JSON params passed to the operation.     |