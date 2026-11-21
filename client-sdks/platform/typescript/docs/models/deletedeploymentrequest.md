# DeleteDeploymentRequest

## Example Usage

```typescript
import { DeleteDeploymentRequest } from "@alienplatform/platform-api/models";

let value: DeleteDeploymentRequest = {
  action: "retain",
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `action`                                                                           | [models.DeleteDeploymentRequestAction](../models/deletedeploymentrequestaction.md) | :heavy_check_mark:                                                                 | N/A                                                                                |
| `closeSubaccount`                                                                  | *boolean*                                                                          | :heavy_minus_sign:                                                                 | Close the subaccount after this last deployment finishes cleanup.                  |