# GenerateManagerTokenResponse

## Example Usage

```typescript
import { GenerateManagerTokenResponse } from "@alienplatform/platform-api/models";

let value: GenerateManagerTokenResponse = {
  accessToken: "<value>",
  expiresIn: 2822.87,
  tokenType: "Bearer",
  managerUrl: "https://responsible-metabolite.name/",
  databaseId: "<id>",
  controlPlaneUrl: "https://fat-lobster.com",
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `accessToken`                                                                                      | *string*                                                                                           | :heavy_check_mark:                                                                                 | Platform JWT for authenticating with the manager                                                   |
| `expiresIn`                                                                                        | *number*                                                                                           | :heavy_check_mark:                                                                                 | Token lifetime in seconds                                                                          |
| `tokenType`                                                                                        | [models.GenerateManagerTokenResponseTokenType](../models/generatemanagertokenresponsetokentype.md) | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `managerUrl`                                                                                       | *string*                                                                                           | :heavy_check_mark:                                                                                 | Manager URL for direct access                                                                      |
| `databaseId`                                                                                       | *string*                                                                                           | :heavy_check_mark:                                                                                 | Log database ID (null if logs not configured)                                                      |
| `controlPlaneUrl`                                                                                  | *string*                                                                                           | :heavy_check_mark:                                                                                 | Log control plane URL (null if logs not configured)                                                |