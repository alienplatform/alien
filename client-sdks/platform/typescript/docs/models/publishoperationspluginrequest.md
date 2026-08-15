# PublishOperationsPluginRequest

## Example Usage

```typescript
import { PublishOperationsPluginRequest } from "@alienplatform/platform-api/models";

let value: PublishOperationsPluginRequest = {
  name: "<value>",
  version: "<value>",
  tier: "destructive",
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `name`                                                                                       | *string*                                                                                     | :heavy_check_mark:                                                                           | Plugin name (from the bundle's metadata.json).                                               |
| `version`                                                                                    | *string*                                                                                     | :heavy_check_mark:                                                                           | Plugin version.                                                                              |
| `tier`                                                                                       | [models.PublishOperationsPluginRequestTier](../models/publishoperationspluginrequesttier.md) | :heavy_check_mark:                                                                           | Plugin-level default risk tier.                                                              |
| `metadata`                                                                                   | *any*                                                                                        | :heavy_minus_sign:                                                                           | The verbatim metadata.json (operations[], binaries{}).                                       |