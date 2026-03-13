# CreateDeploymentTokenResponse

## Example Usage

```typescript
import { CreateDeploymentTokenResponse } from "@aliendotdev/platform-api/models";

let value: CreateDeploymentTokenResponse = {
  token: "<value>",
  deploymentId: "<id>",
};
```

## Fields

| Field                                       | Type                                        | Required                                    | Description                                 |
| ------------------------------------------- | ------------------------------------------- | ------------------------------------------- | ------------------------------------------- |
| `token`                                     | *string*                                    | :heavy_check_mark:                          | The generated agent token (only shown once) |
| `deploymentId`                              | *string*                                    | :heavy_check_mark:                          | The agent ID that this token is scoped to   |