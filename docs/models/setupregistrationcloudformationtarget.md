# SetupRegistrationCloudFormationTarget

## Example Usage

```typescript
import { SetupRegistrationCloudFormationTarget } from "@alienplatform/platform-api/models";

let value: SetupRegistrationCloudFormationTarget = {
  stackId: "<id>",
  requestId: "<id>",
  logicalResourceId: "<id>",
  responseUrl: "https://soulful-best-seller.com/",
};
```

## Fields

| Field                   | Type                    | Required                | Description             |
| ----------------------- | ----------------------- | ----------------------- | ----------------------- |
| `stackId`               | *string*                | :heavy_check_mark:      | N/A                     |
| `requestId`             | *string*                | :heavy_check_mark:      | N/A                     |
| `logicalResourceId`     | *string*                | :heavy_check_mark:      | N/A                     |
| `responseUrl`           | *string*                | :heavy_check_mark:      | N/A                     |
| `physicalResourceId`    | *string*                | :heavy_minus_sign:      | N/A                     |
| `serviceTimeoutSeconds` | *number*                | :heavy_minus_sign:      | N/A                     |