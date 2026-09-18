# PrepareDeploymentCredentialRotationRequest

## Example Usage

```typescript
import { PrepareDeploymentCredentialRotationRequest } from "@alienplatform/platform-api/models/operations";

let value: PrepareDeploymentCredentialRotationRequest = {
  id: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  requestBody: {
    expectedRevision: 250125,
  },
};
```

## Fields

| Field                                                                                                                                  | Type                                                                                                                                   | Required                                                                                                                               | Description                                                                                                                            | Example                                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                                                   | *string*                                                                                                                               | :heavy_check_mark:                                                                                                                     | Unique identifier for the deployment.                                                                                                  | dep_0c29fq4a2yjb7kx3smwdgxlc                                                                                                           |
| `requestBody`                                                                                                                          | [operations.PrepareDeploymentCredentialRotationRequestBody](../../models/operations/preparedeploymentcredentialrotationrequestbody.md) | :heavy_check_mark:                                                                                                                     | N/A                                                                                                                                    |                                                                                                                                        |