# TargetDeploymentBaseImage

Base image metadata for the Horizon machine image.

## Example Usage

```typescript
import { TargetDeploymentBaseImage } from "@alienplatform/platform-api/models";

let value: TargetDeploymentBaseImage = {
  name: "<value>",
  version: "<value>",
};
```

## Fields

| Field                             | Type                              | Required                          | Description                       |
| --------------------------------- | --------------------------------- | --------------------------------- | --------------------------------- |
| `name`                            | *string*                          | :heavy_check_mark:                | Base OS image name.               |
| `version`                         | *string*                          | :heavy_check_mark:                | Base OS image version or channel. |