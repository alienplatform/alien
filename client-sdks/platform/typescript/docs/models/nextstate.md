# NextState

Represents the collective state of all resources in a stack, including platform and pending actions.

## Example Usage

```typescript
import { NextState } from "@alienplatform/platform-api/models";

let value: NextState = {
  platform: "kubernetes",
  resourcePrefix: "<value>",
  resources: {},
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `platform`                                                                 | [models.EventPlatform](../models/eventplatform.md)                         | :heavy_check_mark:                                                         | Represents the target cloud platform.                                      |
| `resourcePrefix`                                                           | *string*                                                                   | :heavy_check_mark:                                                         | A prefix used for resource naming to ensure uniqueness across deployments. |
| `resources`                                                                | Record<string, [models.EventResources](../models/eventresources.md)>       | :heavy_check_mark:                                                         | The state of individual resources, keyed by resource ID.                   |