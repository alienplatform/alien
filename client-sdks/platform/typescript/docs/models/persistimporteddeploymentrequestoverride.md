# PersistImportedDeploymentRequestOverride

A permission set that can be applied across different cloud platforms

## Example Usage

```typescript
import { PersistImportedDeploymentRequestOverride } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestOverride = {
  description: "besides once uh-huh annex grimy hm quit while stitcher whether",
  id: "<id>",
  platforms: {},
};
```

## Fields

| Field                                                                                                                      | Type                                                                                                                       | Required                                                                                                                   | Description                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `description`                                                                                                              | *string*                                                                                                                   | :heavy_check_mark:                                                                                                         | Human-readable description of what this permission set allows                                                              |
| `id`                                                                                                                       | *string*                                                                                                                   | :heavy_check_mark:                                                                                                         | Unique identifier for the permission set (e.g., "storage/data-read")                                                       |
| `platforms`                                                                                                                | [models.PersistImportedDeploymentRequestOverridePlatforms](../models/persistimporteddeploymentrequestoverrideplatforms.md) | :heavy_check_mark:                                                                                                         | Platform-specific permission configurations                                                                                |