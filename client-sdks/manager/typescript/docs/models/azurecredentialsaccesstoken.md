# AzureCredentialsAccessToken

Direct access token

## Example Usage

```typescript
import { AzureCredentialsAccessToken } from "@alienplatform/manager-api/models";

let value: AzureCredentialsAccessToken = {
  token: "<value>",
  type: "accessToken",
};
```

## Fields

| Field                                      | Type                                       | Required                                   | Description                                |
| ------------------------------------------ | ------------------------------------------ | ------------------------------------------ | ------------------------------------------ |
| `token`                                    | *string*                                   | :heavy_check_mark:                         | The bearer token to use for authentication |
| `type`                                     | *"accessToken"*                            | :heavy_check_mark:                         | N/A                                        |