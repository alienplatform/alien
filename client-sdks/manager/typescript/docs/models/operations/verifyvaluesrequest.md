# VerifyValuesRequest

## Example Usage

```typescript
import { VerifyValuesRequest } from "@alienplatform/manager-api/models/operations";

let value: VerifyValuesRequest = {
  scope: "<value>",
  verifyValuesRequest: {
    values: [
      {
        slot: "<value>",
        value: "<value>",
      },
    ],
  },
};
```

## Fields

| Field                                                             | Type                                                              | Required                                                          | Description                                                       |
| ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- |
| `scope`                                                           | *string*                                                          | :heavy_check_mark:                                                | dep:<deploymentId>                                                |
| `verifyValuesRequest`                                             | [models.VerifyValuesRequest](../../models/verifyvaluesrequest.md) | :heavy_check_mark:                                                | N/A                                                               |
