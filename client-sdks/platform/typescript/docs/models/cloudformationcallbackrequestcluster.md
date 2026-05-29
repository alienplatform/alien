# CloudFormationCallbackRequestCluster

Kubernetes cluster setup settings.

## Example Usage

```typescript
import { CloudFormationCallbackRequestCluster } from "@alienplatform/platform-api/models";

let value: CloudFormationCallbackRequestCluster = {
  ownership: "existing",
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `cloud`                                                                                              | *models.CloudFormationCallbackRequestCloudUnion*                                                     | :heavy_minus_sign:                                                                                   | N/A                                                                                                  |
| `namespace`                                                                                          | *string*                                                                                             | :heavy_minus_sign:                                                                                   | Namespace where the Alien chart and application resources run.                                       |
| `ownership`                                                                                          | [models.CloudFormationCallbackRequestOwnership](../models/cloudformationcallbackrequestownership.md) | :heavy_check_mark:                                                                                   | Ownership model for the Kubernetes cluster.                                                          |