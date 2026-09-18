# CreateDeploymentTokenResponse

## Example Usage

```typescript
import { CreateDeploymentTokenResponse } from "@alienplatform/platform-api/models";

let value: CreateDeploymentTokenResponse = {
  token: "<value>",
  deploymentId: "<id>",
};
```

## Fields

| Field                                            | Type                                             | Required                                         | Description                                      |
| ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ |
| `token`                                          | *string*                                         | :heavy_check_mark:                               | The generated deployment token (only shown once) |
| `deploymentId`                                   | *string*                                         | :heavy_check_mark:                               | The deployment ID that this token is scoped to   |