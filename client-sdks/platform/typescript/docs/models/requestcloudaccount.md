# RequestCloudAccount

Create a dedicated account and start this deployment when it is ready.

## Example Usage

```typescript
import { RequestCloudAccount } from "@alienplatform/platform-api/models";

let value: RequestCloudAccount = {
  requestId: "cbb1be14-34b4-43c5-a84e-31c10e33c1f1",
  connectionId: "<id>",
  name: "<value>",
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `requestId`                                              | *string*                                                 | :heavy_check_mark:                                       | Stable request ID. Reuse when retrying the same request. |
| `connectionId`                                           | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `name`                                                   | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `email`                                                  | *string*                                                 | :heavy_minus_sign:                                       | N/A                                                      |