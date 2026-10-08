# SetOperationsPluginEnabledRequest

## Example Usage

```typescript
import { SetOperationsPluginEnabledRequest } from "@alienplatform/platform-api/models/operations";

let value: SetOperationsPluginEnabledRequest = {
  name: "<value>",
  project: "<value>",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `name`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | Plugin name.                                                                                  |
| `project`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | Filter by project ID or name.                                                                 |
| `setOperationsPluginEnabledRequest`                                                           | [models.SetOperationsPluginEnabledRequest](../../models/setoperationspluginenabledrequest.md) | :heavy_minus_sign:                                                                            | N/A                                                                                           |