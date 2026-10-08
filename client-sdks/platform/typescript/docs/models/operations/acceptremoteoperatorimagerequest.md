# AcceptRemoteOperatorImageRequest

## Example Usage

```typescript
import { AcceptRemoteOperatorImageRequest } from "@alienplatform/platform-api/models/operations";

let value: AcceptRemoteOperatorImageRequest = {
  idOrName: "<value>",
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        | Example                                                                                                            |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `idOrName`                                                                                                         | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | Project ID or name.                                                                                                |                                                                                                                    |
| `deploymentId`                                                                                                     | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | Unique identifier for the deployment.                                                                              | dep_0c29fq4a2yjb7kx3smwdgxlc                                                                                       |
| `requestBody`                                                                                                      | [operations.AcceptRemoteOperatorImageRequestBody](../../models/operations/acceptremoteoperatorimagerequestbody.md) | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |                                                                                                                    |