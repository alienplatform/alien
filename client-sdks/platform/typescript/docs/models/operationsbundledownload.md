# OperationsBundleDownload

## Example Usage

```typescript
import { OperationsBundleDownload } from "@alienplatform/platform-api/models";

let value: OperationsBundleDownload = {
  plugin: "<value>",
  pluginVersion: "<value>",
  url: "https://those-mobility.name",
};
```

## Fields

| Field                                                                                                                            | Type                                                                                                                             | Required                                                                                                                         | Description                                                                                                                      |
| -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `plugin`                                                                                                                         | *string*                                                                                                                         | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |
| `pluginVersion`                                                                                                                  | *string*                                                                                                                         | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |
| `url`                                                                                                                            | *string*                                                                                                                         | :heavy_check_mark:                                                                                                               | Presigned URL to GET the bundle ZIP from. Short-lived.                                                                           |
| `env`                                                                                                                            | Record<string, *string*>                                                                                                         | :heavy_minus_sign:                                                                                                               | Environment the plugin process runs with: its settings, which may hold secrets. Never persisted.                                 |
| `envSlots`                                                                                                                       | Record<string, [models.EnvSlots](../models/envslots.md)>                                                                         | :heavy_minus_sign:                                                                                                               | Settings whose value the deployment's manager holds, by environment variable name. The manager fills them in from its own store. |