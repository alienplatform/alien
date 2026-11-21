# SyncAcquireResponseDeploymentClustersBaseImage

Base image metadata for the machine image.

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentClustersBaseImage } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentClustersBaseImage = {
  name: "<value>",
  version: "<value>",
};
```

## Fields

| Field                             | Type                              | Required                          | Description                       |
| --------------------------------- | --------------------------------- | --------------------------------- | --------------------------------- |
| `name`                            | *string*                          | :heavy_check_mark:                | Base OS image name.               |
| `version`                         | *string*                          | :heavy_check_mark:                | Base OS image version or channel. |