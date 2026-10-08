# EcsBootstrapWriteCheckRequest

## Example Usage

```typescript
import { EcsBootstrapWriteCheckRequest } from "@alienplatform/platform-api/models";

let value: EcsBootstrapWriteCheckRequest = {
  stackName: "<value>",
  accountId: "<id>",
  region: "<value>",
  clusterArn: "<value>",
};
```

## Fields

| Field                                                 | Type                                                  | Required                                              | Description                                           |
| ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- |
| `stackName`                                           | *string*                                              | :heavy_check_mark:                                    | N/A                                                   |
| `accountId`                                           | *string*                                              | :heavy_check_mark:                                    | Exact AWS account that owns the Remote Operator stack |
| `region`                                              | *string*                                              | :heavy_check_mark:                                    | Exact AWS Region for the Remote Operator stack        |
| `clusterArn`                                          | *string*                                              | :heavy_check_mark:                                    | Exact ARN of the customer-owned ECS cluster           |
