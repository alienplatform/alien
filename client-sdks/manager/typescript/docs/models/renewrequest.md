# RenewRequest

## Example Usage

```typescript
import { RenewRequest } from "@alienplatform/manager-api/models";

let value: RenewRequest = {
  deploymentId: "<id>",
  session: "<value>",
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `deploymentId`                                       | *string*                                             | :heavy_check_mark:                                   | N/A                                                  |
| `executionClaim`                                     | [models.ExecutionClaim](../models/executionclaim.md) | :heavy_minus_sign:                                   | N/A                                                  |
| `session`                                            | *string*                                             | :heavy_check_mark:                                   | N/A                                                  |