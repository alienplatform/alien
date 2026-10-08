# ListOperationsPluginsResponse

## Example Usage

```typescript
import { ListOperationsPluginsResponse } from "@alienplatform/platform-api/models";

let value: ListOperationsPluginsResponse = {
  plugins: [],
  installations: {
    total: 444088,
    ready: 858107,
    syncing: 914515,
    stuck: 583858,
  },
};
```

## Fields

| Field                                                                                                                           | Type                                                                                                                            | Required                                                                                                                        | Description                                                                                                                     |
| ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| `plugins`                                                                                                                       | [models.OperationsPlugin](../models/operationsplugin.md)[]                                                                      | :heavy_check_mark:                                                                                                              | N/A                                                                                                                             |
| `installations`                                                                                                                 | [models.OperatorInstallationsRollup](../models/operatorinstallationsrollup.md)                                                  | :heavy_check_mark:                                                                                                              | Rollup of this project's pull-mode installations by operations-bundle sync status, reflecting the currently enabled plugin set. |