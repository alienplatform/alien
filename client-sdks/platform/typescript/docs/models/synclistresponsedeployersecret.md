# SyncListResponseDeployerSecret

The state of one deployer secret slot, reported with the deployment.

## Example Usage

```typescript
import { SyncListResponseDeployerSecret } from "@alienplatform/platform-api/models";

let value: SyncListResponseDeployerSecret = {
  inputId: "<id>",
  label: "<value>",
  location: {
    cliCommand: "<value>",
    name: "<value>",
    store: "azure-key-vault",
  },
  required: true,
  status: "invalid",
};
```

## Fields

| Field                                                                                                                                    | Type                                                                                                                                     | Required                                                                                                                                 | Description                                                                                                                              |
| ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `inputId`                                                                                                                                | *string*                                                                                                                                 | :heavy_check_mark:                                                                                                                       | Stack input id.                                                                                                                          |
| `label`                                                                                                                                  | *string*                                                                                                                                 | :heavy_check_mark:                                                                                                                       | The input's label.                                                                                                                       |
| `location`                                                                                                                               | [models.SyncListResponseLocation](../models/synclistresponselocation.md)                                                                 | :heavy_check_mark:                                                                                                                       | Where a deployer writes a vault-native secret.                                                                                           |
| `message`                                                                                                                                | *string*                                                                                                                                 | :heavy_minus_sign:                                                                                                                       | Why the slot is invalid.                                                                                                                 |
| `required`                                                                                                                               | *boolean*                                                                                                                                | :heavy_check_mark:                                                                                                                       | Whether the workload cannot start without it.                                                                                            |
| `status`                                                                                                                                 | [models.SyncListResponseRuntimeMetadataStatus](../models/synclistresponseruntimemetadatastatus.md)                                       | :heavy_check_mark:                                                                                                                       | Whether a deployer secret slot holds a usable value. Alien learns this from<br/>metadata only and never reads the value.                 |
| `version`                                                                                                                                | *string*                                                                                                                                 | :heavy_minus_sign:                                                                                                                       | The secret store's version of the present value (never the value or a<br/>hash of it). A new version reaches workloads with the next update. |
