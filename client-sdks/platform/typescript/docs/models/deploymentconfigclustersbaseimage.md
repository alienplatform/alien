# DeploymentConfigClustersBaseImage

Base image metadata for the machine image.

## Example Usage

```typescript
import { DeploymentConfigClustersBaseImage } from "@alienplatform/platform-api/models";

let value: DeploymentConfigClustersBaseImage = {
  name: "<value>",
  version: "<value>",
};
```

## Fields

| Field                             | Type                              | Required                          | Description                       |
| --------------------------------- | --------------------------------- | --------------------------------- | --------------------------------- |
| `name`                            | *string*                          | :heavy_check_mark:                | Base OS image name.               |
| `version`                         | *string*                          | :heavy_check_mark:                | Base OS image version or channel. |