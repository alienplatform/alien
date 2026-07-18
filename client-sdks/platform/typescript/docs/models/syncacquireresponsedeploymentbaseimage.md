# SyncAcquireResponseDeploymentBaseImage

Base image metadata for the Horizon machine image.

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentBaseImage } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentBaseImage = {
  name: "<value>",
  version: "<value>",
};
```

## Fields

| Field                             | Type                              | Required                          | Description                       |
| --------------------------------- | --------------------------------- | --------------------------------- | --------------------------------- |
| `name`                            | *string*                          | :heavy_check_mark:                | Base OS image name.               |
| `version`                         | *string*                          | :heavy_check_mark:                | Base OS image version or channel. |