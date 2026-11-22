# UnacquiredDeployment

## Example Usage

```typescript
import { UnacquiredDeployment } from "@alienplatform/platform-api/models";

let value: UnacquiredDeployment = {
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  reason: "deploymentModelMismatch",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `deploymentId`                                                                                | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the deployment.                                                         | dep_0c29fq4a2yjb7kx3smwdgxlc                                                                  |
| `reason`                                                                                      | [models.DeploymentAcquireUnavailableReason](../models/deploymentacquireunavailablereason.md)  | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `retryAfter`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | N/A                                                                                           |                                                                                               |