# ResolveBindingRequest

Request body for `POST /v1/bindings/resolve`.

## Example Usage

```typescript
import { ResolveBindingRequest } from "@alienplatform/manager-api/models";

let value: ResolveBindingRequest = {
  deploymentId: "<id>",
  resourceId: "<id>",
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `deploymentId`                                               | *string*                                                     | :heavy_check_mark:                                           | Deployment containing the remote-enabled resource.           |
| `resourceId`                                                 | *string*                                                     | :heavy_check_mark:                                           | Logical Storage resource id in the deployment's stack state. |