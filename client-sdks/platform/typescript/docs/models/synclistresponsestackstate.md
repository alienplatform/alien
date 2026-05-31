# SyncListResponseStackState

State of infrastructure components managed by this deployment

## Example Usage

```typescript
import { SyncListResponseStackState } from "@alienplatform/platform-api/models";

let value: SyncListResponseStackState = {
  platform: "aws",
  resourcePrefix: "<value>",
  resources: {},
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `platform`                                                                                                     | [models.SyncListResponseStackStatePlatform](../models/synclistresponsestackstateplatform.md)                   | :heavy_check_mark:                                                                                             | Represents the target cloud platform.                                                                          |
| `resourcePrefix`                                                                                               | *string*                                                                                                       | :heavy_check_mark:                                                                                             | A prefix used for resource naming to ensure uniqueness across deployments.                                     |
| `resources`                                                                                                    | Record<string, [models.SyncListResponseStackStateResources](../models/synclistresponsestackstateresources.md)> | :heavy_check_mark:                                                                                             | The state of individual resources, keyed by resource ID.                                                       |