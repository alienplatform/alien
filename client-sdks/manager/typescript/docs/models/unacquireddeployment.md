# UnacquiredDeployment

Atomic outcome for one explicitly requested deployment that was not acquired.

## Example Usage

```typescript
import { UnacquiredDeployment } from "@alienplatform/manager-api/models";

let value: UnacquiredDeployment = {
  deploymentId: "<id>",
  reason: "deploymentModelMismatch",
};
```

## Fields

| Field                                                                                                                                                          | Type                                                                                                                                                           | Required                                                                                                                                                       | Description                                                                                                                                                    |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `deploymentId`                                                                                                                                                 | *string*                                                                                                                                                       | :heavy_check_mark:                                                                                                                                             | N/A                                                                                                                                                            |
| `reason`                                                                                                                                                       | [models.DeploymentAcquireUnavailableReason](../models/deploymentacquireunavailablereason.md)                                                                   | :heavy_check_mark:                                                                                                                                             | Why an explicitly requested deployment was not acquired.<br/><br/>These reasons are intentionally bounded and do not identify the session<br/>that owns a competing lease. |
| `retryAfter`                                                                                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)                                                                  | :heavy_minus_sign:                                                                                                                                             | N/A                                                                                                                                                            |