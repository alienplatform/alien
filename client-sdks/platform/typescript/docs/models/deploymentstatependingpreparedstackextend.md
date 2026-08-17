# DeploymentStatePendingPreparedStackExtend

A permission set that can be applied across different cloud platforms

## Example Usage

```typescript
import { DeploymentStatePendingPreparedStackExtend } from "@alienplatform/platform-api/models";

let value: DeploymentStatePendingPreparedStackExtend = {
  description:
    "typewriter questionably next inquisitively yahoo shoulder task grave now",
  id: "<id>",
  platforms: {},
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `description`                                                                                                                | *string*                                                                                                                     | :heavy_check_mark:                                                                                                           | Human-readable description of what this permission set allows                                                                |
| `id`                                                                                                                         | *string*                                                                                                                     | :heavy_check_mark:                                                                                                           | Unique identifier for the permission set (e.g., "storage/data-read")                                                         |
| `platforms`                                                                                                                  | [models.DeploymentStatePendingPreparedStackExtendPlatforms](../models/deploymentstatependingpreparedstackextendplatforms.md) | :heavy_check_mark:                                                                                                           | Platform-specific permission configurations                                                                                  |