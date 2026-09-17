# DeleteDeploymentResponse

## Example Usage

```typescript
import { DeleteDeploymentResponse } from "@alienplatform/manager-api/models";

let value: DeleteDeploymentResponse = {
  action: "detach",
  message: "<value>",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `action`                                                             | [models.DeleteDeploymentAction](../models/deletedeploymentaction.md) | :heavy_check_mark:                                                   | N/A                                                                  |
| `cleanupRequired`                                                    | *boolean*                                                            | :heavy_minus_sign:                                                   | Whether the caller must continue deleting setup-owned resources.     |
| `message`                                                            | *string*                                                             | :heavy_check_mark:                                                   | Human-readable summary of the accepted operation.                    |