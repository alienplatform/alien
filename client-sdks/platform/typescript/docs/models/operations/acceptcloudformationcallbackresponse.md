# AcceptCloudFormationCallbackResponse

CloudFormation callback accepted.

## Example Usage

```typescript
import { AcceptCloudFormationCallbackResponse } from "@alienplatform/platform-api/models/operations";

let value: AcceptCloudFormationCallbackResponse = {
  callbackOperationId: "<id>",
  physicalResourceId: "<id>",
  deploymentId: "<id>",
};
```

## Fields

| Field                 | Type                  | Required              | Description           |
| --------------------- | --------------------- | --------------------- | --------------------- |
| `callbackOperationId` | *string*              | :heavy_check_mark:    | N/A                   |
| `physicalResourceId`  | *string*              | :heavy_check_mark:    | N/A                   |
| `deploymentId`        | *string*              | :heavy_check_mark:    | N/A                   |