# UpdateAPIKeyRequest

Request schema for updating an API key

## Example Usage

```typescript
import { UpdateAPIKeyRequest } from "@alienplatform/platform-api/models";

let value: UpdateAPIKeyRequest = {};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `enabled`                                                                                          | *boolean*                                                                                          | :heavy_minus_sign:                                                                                 | N/A                                                                                                |
| `description`                                                                                      | *string*                                                                                           | :heavy_minus_sign:                                                                                 | N/A                                                                                                |
| `deploymentSetupConfig`                                                                            | [models.UpdateDeploymentSetupPolicy](../models/updatedeploymentsetuppolicy.md)                     | :heavy_minus_sign:                                                                                 | Editable part of a deployment link's setup config. Locked env vars and input values are preserved. |
| `expiresAt`                                                                                        | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)      | :heavy_minus_sign:                                                                                 | Optional expiration date for the API key                                                           |