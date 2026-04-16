# DeploymentDetailResponseStackState

State of infrastructure components managed by this deployment

## Example Usage

```typescript
import { DeploymentDetailResponseStackState } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseStackState = {
  platform: "gcp",
  resourcePrefix: "<value>",
  resources: {},
};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `platform`                                                                                                                     | [models.DeploymentDetailResponseStackStatePlatform](../models/deploymentdetailresponsestackstateplatform.md)                   | :heavy_check_mark:                                                                                                             | Represents the target cloud platform.                                                                                          |
| `resourcePrefix`                                                                                                               | *string*                                                                                                                       | :heavy_check_mark:                                                                                                             | A prefix used for resource naming to ensure uniqueness across deployments.                                                     |
| `resources`                                                                                                                    | Record<string, [models.DeploymentDetailResponseStackStateResources](../models/deploymentdetailresponsestackstateresources.md)> | :heavy_check_mark:                                                                                                             | The state of individual resources, keyed by resource ID.                                                                       |