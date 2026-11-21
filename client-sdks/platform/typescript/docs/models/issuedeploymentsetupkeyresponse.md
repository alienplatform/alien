# IssueDeploymentSetupKeyResponse

## Example Usage

```typescript
import { IssueDeploymentSetupKeyResponse } from "@alienplatform/platform-api/models";

let value: IssueDeploymentSetupKeyResponse = {
  token: "<value>",
  expiresAt: new Date("2026-10-03T00:57:17.821Z"),
};
```

## Fields

| Field                                                                                                     | Type                                                                                                      | Required                                                                                                  | Description                                                                                               |
| --------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| `token`                                                                                                   | *string*                                                                                                  | :heavy_check_mark:                                                                                        | The setup key. Shown once; set it as TF_VAR_token.                                                        |
| `expiresAt`                                                                                               | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)             | :heavy_check_mark:                                                                                        | When the key stops working (two hours).                                                                   |
| `managementExternalId`                                                                                    | *string*                                                                                                  | :heavy_minus_sign:                                                                                        | The ExternalId the module's management_external_id variable needs (EKS), when the deployment has a group. |