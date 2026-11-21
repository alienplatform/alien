# GetDeploymentCredentialRotationValuesRequest

## Example Usage

```typescript
import { GetDeploymentCredentialRotationValuesRequest } from "@alienplatform/platform-api/models/operations";

let value: GetDeploymentCredentialRotationValuesRequest = {
  id: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  requestBody: {
    rotationId: "<id>",
  },
};
```

## Fields

| Field                                                                                                                                      | Type                                                                                                                                       | Required                                                                                                                                   | Description                                                                                                                                | Example                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ |
| `id`                                                                                                                                       | *string*                                                                                                                                   | :heavy_check_mark:                                                                                                                         | Unique identifier for the deployment.                                                                                                      | dep_0c29fq4a2yjb7kx3smwdgxlc                                                                                                               |
| `requestBody`                                                                                                                              | [operations.GetDeploymentCredentialRotationValuesRequestBody](../../models/operations/getdeploymentcredentialrotationvaluesrequestbody.md) | :heavy_check_mark:                                                                                                                         | N/A                                                                                                                                        |                                                                                                                                            |