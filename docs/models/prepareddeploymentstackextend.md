# PreparedDeploymentStackExtend

A permission set that can be applied across different cloud platforms

## Example Usage

```typescript
import { PreparedDeploymentStackExtend } from "@alienplatform/platform-api/models";

let value: PreparedDeploymentStackExtend = {
  description: "yet rise solidly than exalted hearten flawed gosh",
  id: "<id>",
  platforms: {},
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `description`                                                                                        | *string*                                                                                             | :heavy_check_mark:                                                                                   | Human-readable description of what this permission set allows                                        |
| `id`                                                                                                 | *string*                                                                                             | :heavy_check_mark:                                                                                   | Unique identifier for the permission set (e.g., "storage/data-read")                                 |
| `platforms`                                                                                          | [models.PreparedDeploymentStackExtendPlatforms](../models/prepareddeploymentstackextendplatforms.md) | :heavy_check_mark:                                                                                   | Platform-specific permission configurations                                                          |