# PutValuesRequest

## Example Usage

```typescript
import { PutValuesRequest } from "@alienplatform/manager-api/models/operations";

let value: PutValuesRequest = {
  scope: "<value>",
  putValuesRequest: {
    values: [
      {
        kind: "plain",
        slot: "<value>",
        value: "<value>",
      },
    ],
  },
};
```

## Fields

| Field                                                       | Type                                                        | Required                                                    | Description                                                 |
| ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- |
| `scope`                                                     | *string*                                                    | :heavy_check_mark:                                          | dep:<deploymentId>                                          |
| `ifNoneMatch`                                               | *string*                                                    | :heavy_minus_sign:                                          | `*` writes only when the slot is empty                      |
| `putValuesRequest`                                          | [models.PutValuesRequest](../../models/putvaluesrequest.md) | :heavy_check_mark:                                          | N/A                                                         |
