# SetupLocation

Where this Private Alien Instance's setup was installed. Returned when retrieving one manager.

## Example Usage

```typescript
import { SetupLocation } from "@alienplatform/platform-api/models";

let value: SetupLocation = {};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `stackId`                                          | *string*                                           | :heavy_minus_sign:                                 | CloudFormation stack ARN, for CloudFormation setup |
| `stackName`                                        | *string*                                           | :heavy_minus_sign:                                 | CloudFormation stack name                          |
| `region`                                           | *string*                                           | :heavy_minus_sign:                                 | Region of the stack                                |