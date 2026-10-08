# CreateSetupRegistrationOperationRequestCluster

Kubernetes cluster setup settings.

## Example Usage

```typescript
import { CreateSetupRegistrationOperationRequestCluster } from "@alienplatform/platform-api/models";

let value: CreateSetupRegistrationOperationRequestCluster = {
  ownership: "existing",
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `cloud`                                                                                                                  | *models.CreateSetupRegistrationOperationRequestCloudUnion*                                                               | :heavy_minus_sign:                                                                                                       | N/A                                                                                                                      |
| `namespace`                                                                                                              | *string*                                                                                                                 | :heavy_minus_sign:                                                                                                       | Namespace where the Alien chart and application resources run.                                                           |
| `ownership`                                                                                                              | [models.CreateSetupRegistrationOperationRequestOwnership](../models/createsetupregistrationoperationrequestownership.md) | :heavy_check_mark:                                                                                                       | Ownership model for the Kubernetes cluster.                                                                              |