# PublishOperationsPluginRequest

## Example Usage

```typescript
import { PublishOperationsPluginRequest } from "@alienplatform/platform-api/models";

let value: PublishOperationsPluginRequest = {
  name: "<value>",
  version: "<value>",
  uploadId: "<id>",
  tier: "destructive",
  metadata: {
    name: "<value>",
    version: "<value>",
    binaries: {
      arm64: "<value>",
    },
  },
};
```

## Fields

| Field                                                                                                                                                                                                | Type                                                                                                                                                                                                 | Required                                                                                                                                                                                             | Description                                                                                                                                                                                          |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `name`                                                                                                                                                                                               | *string*                                                                                                                                                                                             | :heavy_check_mark:                                                                                                                                                                                   | Plugin name (from the bundle's metadata.json).                                                                                                                                                       |
| `version`                                                                                                                                                                                            | *string*                                                                                                                                                                                             | :heavy_check_mark:                                                                                                                                                                                   | Plugin version.                                                                                                                                                                                      |
| `uploadId`                                                                                                                                                                                           | *string*                                                                                                                                                                                             | :heavy_check_mark:                                                                                                                                                                                   | The uploadId returned by POST /plugins/upload-url for the ZIP just uploaded. Identifies the exact S3 object to publish — never derived from name/version, so it can't collide with any other upload. |
| `tier`                                                                                                                                                                                               | [models.PublishOperationsPluginRequestTier](../models/publishoperationspluginrequesttier.md)                                                                                                         | :heavy_check_mark:                                                                                                                                                                                   | Plugin-level default risk tier.                                                                                                                                                                      |
| `metadata`                                                                                                                                                                                           | [models.Metadata](../models/metadata.md)                                                                                                                                                             | :heavy_check_mark:                                                                                                                                                                                   | The complete canonical metadata.json from the uploaded bundle.                                                                                                                                       |