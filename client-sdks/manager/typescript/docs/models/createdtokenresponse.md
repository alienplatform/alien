# CreatedTokenResponse

## Example Usage

```typescript
import { CreatedTokenResponse } from "@alienplatform/manager-api/models";

let value: CreatedTokenResponse = {
  createdAt: "1722816104219",
  id: "<id>",
  keyPrefix: "<value>",
  tokenType: "<value>",
  token: "<value>",
};
```

## Fields

| Field                                               | Type                                                | Required                                            | Description                                         |
| --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- |
| `createdAt`                                         | *string*                                            | :heavy_check_mark:                                  | N/A                                                 |
| `deploymentGroupId`                                 | *string*                                            | :heavy_minus_sign:                                  | N/A                                                 |
| `deploymentId`                                      | *string*                                            | :heavy_minus_sign:                                  | N/A                                                 |
| `id`                                                | *string*                                            | :heavy_check_mark:                                  | N/A                                                 |
| `keyPrefix`                                         | *string*                                            | :heavy_check_mark:                                  | N/A                                                 |
| `tokenType`                                         | *string*                                            | :heavy_check_mark:                                  | N/A                                                 |
| `token`                                             | *string*                                            | :heavy_check_mark:                                  | The raw token. Shown once; only its hash is stored. |