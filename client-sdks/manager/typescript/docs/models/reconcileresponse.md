# ReconcileResponse

## Example Usage

```typescript
import { ReconcileResponse } from "@alienplatform/manager-api/models";

let value: ReconcileResponse = {
  current: "<value>",
  success: true,
};
```

## Fields

| Field                                                                                                                 | Type                                                                                                                  | Required                                                                                                              | Description                                                                                                           |
| --------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| `current`                                                                                                             | *any*                                                                                                                 | :heavy_check_mark:                                                                                                    | N/A                                                                                                                   |
| `nativeImageHost`                                                                                                     | *string*                                                                                                              | :heavy_minus_sign:                                                                                                    | Native image registry host for Lambda/Cloud Run.<br/>Returned so push clients can set it on their local DeploymentConfig. |
| `success`                                                                                                             | *boolean*                                                                                                             | :heavy_check_mark:                                                                                                    | N/A                                                                                                                   |