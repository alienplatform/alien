# ListOperationsPluginsResponse

## Example Usage

```typescript
import { ListOperationsPluginsResponse } from "@alienplatform/platform-api/models";

let value: ListOperationsPluginsResponse = {
  plugins: [],
};
```

## Fields

| Field                                                                                                                           | Type                                                                                                                            | Required                                                                                                                        | Description                                                                                                                     |
| ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| `plugins`                                                                                                                       | [models.OperationsPlugin](../models/operationsplugin.md)[]                                                                      | :heavy_check_mark:                                                                                                              | N/A                                                                                                                             |
| `installations`                                                                                                                 | [models.OperatorInstallationsRollup](../models/operatorinstallationsrollup.md)                                                  | :heavy_minus_sign:                                                                                                              | Rollup of this project's pull-mode installations by operations-bundle sync status, reflecting the currently enabled plugin set. |