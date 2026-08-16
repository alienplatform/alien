# RemoteBindingsExternalAccessResponse

## Example Usage

```typescript
import { RemoteBindingsExternalAccessResponse } from "@alienplatform/platform-api/models";

let value: RemoteBindingsExternalAccessResponse = {
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  resourceId: "<id>",
  accessToken: "<value>",
  expiresIn: 2804.57,
  tokenType: "Bearer",
  managerUrl: "https://pointed-boulevard.info",
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        | Example                                                                                                            |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `deploymentId`                                                                                                     | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | Unique identifier for the deployment.                                                                              | dep_0c29fq4a2yjb7kx3smwdgxlc                                                                                       |
| `resourceId`                                                                                                       | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |                                                                                                                    |
| `accessToken`                                                                                                      | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |                                                                                                                    |
| `expiresIn`                                                                                                        | *number*                                                                                                           | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |                                                                                                                    |
| `tokenType`                                                                                                        | [models.RemoteBindingsExternalAccessResponseTokenType](../models/remotebindingsexternalaccessresponsetokentype.md) | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |                                                                                                                    |
| `managerUrl`                                                                                                       | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |                                                                                                                    |