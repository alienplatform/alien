# PersistImportedDeploymentRequestPreparedStackOverride

A permission set that can be applied across different cloud platforms

## Example Usage

```typescript
import { PersistImportedDeploymentRequestPreparedStackOverride } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestPreparedStackOverride = {
  description: "edge bleakly quicker splosh caring nor moor yuck",
  id: "<id>",
  platforms: {},
};
```

## Fields

| Field                                                                                                                                                | Type                                                                                                                                                 | Required                                                                                                                                             | Description                                                                                                                                          |
| ---------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| `description`                                                                                                                                        | *string*                                                                                                                                             | :heavy_check_mark:                                                                                                                                   | Human-readable description of what this permission set allows                                                                                        |
| `id`                                                                                                                                                 | *string*                                                                                                                                             | :heavy_check_mark:                                                                                                                                   | Unique identifier for the permission set (e.g., "storage/data-read")                                                                                 |
| `platforms`                                                                                                                                          | [models.PersistImportedDeploymentRequestPreparedStackOverridePlatforms](../models/persistimporteddeploymentrequestpreparedstackoverrideplatforms.md) | :heavy_check_mark:                                                                                                                                   | Platform-specific permission configurations                                                                                                          |
