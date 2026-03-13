# Failure

## Example Usage

```typescript
import { Failure } from "@alienplatform/platform-api/models";

let value: Failure = {
  deploymentId: "ag_pnj2da55wi5sxbdcav9t273je",
  projectId: "<id>",
  error: {
    code: "<value>",
    message: "<value>",
    internal: true,
  },
};
```

## Fields

| Field                                    | Type                                     | Required                                 | Description                              | Example                                  |
| ---------------------------------------- | ---------------------------------------- | ---------------------------------------- | ---------------------------------------- | ---------------------------------------- |
| `deploymentId`                           | *string*                                 | :heavy_check_mark:                       | ID of the deployment that failed         | ag_pnj2da55wi5sxbdcav9t273je             |
| `projectId`                              | *string*                                 | :heavy_check_mark:                       | Project ID the agent belongs to          |                                          |
| `error`                                  | [models.APIError](../models/apierror.md) | :heavy_check_mark:                       | N/A                                      |                                          |