# PutExternalAIBindingRequestDatabricks

## Example Usage

```typescript
import { PutExternalAIBindingRequestDatabricks } from "@alienplatform/platform-api/models";

let value: PutExternalAIBindingRequestDatabricks = {
  provider: "databricks",
  workspaceUrl: "https://well-off-pension.biz",
  clientId: "<id>",
  clientSecret: "<value>",
  acknowledgeAlienCredentialAccess: true,
};
```

## Fields

| Field                              | Type                               | Required                           | Description                        |
| ---------------------------------- | ---------------------------------- | ---------------------------------- | ---------------------------------- |
| `provider`                         | *"databricks"*                     | :heavy_check_mark:                 | N/A                                |
| `workspaceUrl`                     | *string*                           | :heavy_check_mark:                 | N/A                                |
| `clientId`                         | *string*                           | :heavy_check_mark:                 | N/A                                |
| `clientSecret`                     | *string*                           | :heavy_check_mark:                 | N/A                                |
| `acknowledgeAlienCredentialAccess` | *boolean*                          | :heavy_check_mark:                 | N/A                                |