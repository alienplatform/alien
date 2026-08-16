# PublishOperationsPluginResponse

## Example Usage

```typescript
import { PublishOperationsPluginResponse } from "@alienplatform/platform-api/models";

let value: PublishOperationsPluginResponse = {
  name: "<value>",
  version: "<value>",
  tier: "read-only",
  enabled: false,
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `name`                                                                                         | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `version`                                                                                      | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `tier`                                                                                         | [models.PublishOperationsPluginResponseTier](../models/publishoperationspluginresponsetier.md) | :heavy_check_mark:                                                                             | How risky an operation is (declared by the plugin metadata).                                   |
| `enabled`                                                                                      | *boolean*                                                                                      | :heavy_check_mark:                                                                             | N/A                                                                                            |