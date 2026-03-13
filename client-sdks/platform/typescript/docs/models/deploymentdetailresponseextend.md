# DeploymentDetailResponseExtend

A permission set that can be applied across different cloud platforms

## Example Usage

```typescript
import { DeploymentDetailResponseExtend } from "@aliendotdev/platform-api/models";

let value: DeploymentDetailResponseExtend = {
  description: "how meanwhile amid permafrost obnoxiously geez stake",
  id: "<id>",
  platforms: {},
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `description`                                                                                          | *string*                                                                                               | :heavy_check_mark:                                                                                     | Human-readable description of what this permission set allows                                          |
| `id`                                                                                                   | *string*                                                                                               | :heavy_check_mark:                                                                                     | Unique identifier for the permission set (e.g., "storage/data-read")                                   |
| `platforms`                                                                                            | [models.DeploymentDetailResponseExtendPlatforms](../models/deploymentdetailresponseextendplatforms.md) | :heavy_check_mark:                                                                                     | Platform-specific permission configurations                                                            |