# ResolveBindingRequest

Request body for `POST /v1/bindings/resolve`.

## Example Usage

```typescript
import { ResolveBindingRequest } from "@alienplatform/manager-api/models";

let value: ResolveBindingRequest = {
  deploymentId: "<id>",
};
```

## Fields

| Field                                                               | Type                                                                | Required                                                            | Description                                                         |
| ------------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------- |
| `deploymentId`                                                      | *string*                                                            | :heavy_check_mark:                                                  | Deployment containing the remote-enabled resource.                  |
| `kind`                                                              | [models.ResolveBindingKind](../models/resolvebindingkind.md)        | :heavy_minus_sign:                                                  | N/A                                                                 |
| `resourceId`                                                        | *string*                                                            | :heavy_minus_sign:                                                  | Logical remote-enabled resource id in the deployment's stack state. |