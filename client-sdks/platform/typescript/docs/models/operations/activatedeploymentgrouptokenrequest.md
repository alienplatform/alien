# ActivateDeploymentGroupTokenRequest

## Example Usage

```typescript
import { ActivateDeploymentGroupTokenRequest } from "@alienplatform/platform-api/models/operations";

let value: ActivateDeploymentGroupTokenRequest = {
  id: "dg_r27ict8c7vcgsumpj90ackf7bnwt",
  linkId: "apikey_ye96yxs1tjnrrwulp8frh6w5t6e0",
};
```

## Fields

| Field                                                                       | Type                                                                        | Required                                                                    | Description                                                                 | Example                                                                     |
| --------------------------------------------------------------------------- | --------------------------------------------------------------------------- | --------------------------------------------------------------------------- | --------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| `id`                                                                        | *string*                                                                    | :heavy_check_mark:                                                          | Unique identifier for the deployment group.                                 | dg_r27ict8c7vcgsumpj90ackf7bnwt                                             |
| `linkId`                                                                    | *string*                                                                    | :heavy_check_mark:                                                          | Unique identifier for the api key.                                          | apikey_ye96yxs1tjnrrwulp8frh6w5t6e0                                         |
| `activateSetupLinkRequest`                                                  | [models.ActivateSetupLinkRequest](../../models/activatesetuplinkrequest.md) | :heavy_minus_sign:                                                          | N/A                                                                         |                                                                             |