# DeploymentSetupCredentialIssuedEvent

## Example Usage

```typescript
import { DeploymentSetupCredentialIssuedEvent } from "@alienplatform/platform-api/models";

let value: DeploymentSetupCredentialIssuedEvent = {
  type: "DeploymentSetupCredentialIssued",
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlcxd7f",
  credentialId: "<id>",
  revokedCredentialIds: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  expiresAt: new Date("2024-08-11T17:31:19.698Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `type`                                                                                        | *"DeploymentSetupCredentialIssued"*                                                           | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `deploymentId`                                                                                | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the deployment.                                                         | dep_0c29fq4a2yjb7kx3smwdgxlcxd7f                                                              |
| `credentialId`                                                                                | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `revokedCredentialIds`                                                                        | *string*[]                                                                                    | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `expiresAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |