# CancelDeploymentCredentialRotationRequest

## Example Usage

```typescript
import { CancelDeploymentCredentialRotationRequest } from "@alienplatform/platform-api/models/operations";

let value: CancelDeploymentCredentialRotationRequest = {
  id: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  requestBody: {
    rotationId: "<id>",
  },
};
```

## Fields

| Field                                                                                                                                | Type                                                                                                                                 | Required                                                                                                                             | Description                                                                                                                          | Example                                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ |
| `id`                                                                                                                                 | *string*                                                                                                                             | :heavy_check_mark:                                                                                                                   | Unique identifier for the deployment.                                                                                                | dep_0c29fq4a2yjb7kx3smwdgxlc                                                                                                         |
| `requestBody`                                                                                                                        | [operations.CancelDeploymentCredentialRotationRequestBody](../../models/operations/canceldeploymentcredentialrotationrequestbody.md) | :heavy_check_mark:                                                                                                                   | N/A                                                                                                                                  |                                                                                                                                      |