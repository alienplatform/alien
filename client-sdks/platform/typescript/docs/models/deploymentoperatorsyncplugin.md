# DeploymentOperatorSyncPlugin

## Example Usage

```typescript
import { DeploymentOperatorSyncPlugin } from "@alienplatform/platform-api/models";

let value: DeploymentOperatorSyncPlugin = {
  name: "<value>",
  version: "<value>",
  source: "custom",
  state: "updating",
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `name`                                                                                       | *string*                                                                                     | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `version`                                                                                    | *string*                                                                                     | :heavy_check_mark:                                                                           | Version the Operator should run                                                              |
| `source`                                                                                     | [models.DeploymentOperatorSyncPluginSource](../models/deploymentoperatorsyncpluginsource.md) | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `state`                                                                                      | [models.DeploymentOperatorSyncPluginState](../models/deploymentoperatorsyncpluginstate.md)   | :heavy_check_mark:                                                                           | What the Operator still has to do for this plugin                                            |
| `previousVersion`                                                                            | *string*                                                                                     | :heavy_minus_sign:                                                                           | Version the Operator is running now (only while updating)                                    |
