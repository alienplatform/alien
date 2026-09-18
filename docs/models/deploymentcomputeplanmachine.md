# DeploymentComputePlanMachine

## Example Usage

```typescript
import { DeploymentComputePlanMachine } from "@alienplatform/platform-api/models";

let value: DeploymentComputePlanMachine = {
  machine: "<value>",
  profile: {
    cpu: "<value>",
    memoryBytes: 718877,
    ephemeralStorageBytes: 953830,
  },
  recommended: true,
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `machine`                                                                        | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `profile`                                                                        | [models.DeploymentComputePlanProfile](../models/deploymentcomputeplanprofile.md) | :heavy_check_mark:                                                               | N/A                                                                              |
| `recommended`                                                                    | *boolean*                                                                        | :heavy_check_mark:                                                               | N/A                                                                              |