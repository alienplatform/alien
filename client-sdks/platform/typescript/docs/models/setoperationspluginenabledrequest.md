# SetOperationsPluginEnabledRequest

## Example Usage

```typescript
import { SetOperationsPluginEnabledRequest } from "@alienplatform/platform-api/models";

let value: SetOperationsPluginEnabledRequest = {
  enabled: false,
};
```

## Fields

| Field                                                       | Type                                                        | Required                                                    | Description                                                 |
| ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- |
| `enabled`                                                   | *boolean*                                                   | :heavy_check_mark:                                          | Whether the plugin should be baked into the operator image. |