# TargetDeploymentClustersBaseImage

Base image metadata for the machine image.

## Example Usage

```typescript
import { TargetDeploymentClustersBaseImage } from "@alienplatform/platform-api/models";

let value: TargetDeploymentClustersBaseImage = {
  name: "<value>",
  version: "<value>",
};
```

## Fields

| Field                             | Type                              | Required                          | Description                       |
| --------------------------------- | --------------------------------- | --------------------------------- | --------------------------------- |
| `name`                            | *string*                          | :heavy_check_mark:                | Base OS image name.               |
| `version`                         | *string*                          | :heavy_check_mark:                | Base OS image version or channel. |