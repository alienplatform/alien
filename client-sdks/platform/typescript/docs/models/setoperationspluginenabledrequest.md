# SetOperationsPluginEnabledRequest

## Example Usage

```typescript
import { SetOperationsPluginEnabledRequest } from "@alienplatform/platform-api/models";

let value: SetOperationsPluginEnabledRequest = {
  enabled: false,
};
```

## Fields

| Field                                                                                     | Type                                                                                      | Required                                                                                  | Description                                                                               |
| ----------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| `enabled`                                                                                 | *boolean*                                                                                 | :heavy_check_mark:                                                                        | Whether the plugin is distributed to this project's Operators.                            |
| `dryRun`                                                                                  | *boolean*                                                                                 | :heavy_minus_sign:                                                                        | Validate the change and return its permission delta without saving it. Defaults to false. |
| `expectedPermissionDiff`                                                                  | [models.OperationsPermissionDiff](../models/operationspermissiondiff.md)                  | :heavy_minus_sign:                                                                        | N/A                                                                                       |