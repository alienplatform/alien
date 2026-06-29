# Machine

## Example Usage

```typescript
import { Machine } from "@alienplatform/platform-api/models";

let value: Machine = {
  machine: "<value>",
  profile: {
    cpu: "<value>",
    memoryBytes: 325948,
    ephemeralStorageBytes: 794662,
  },
  recommended: false,
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `machine`                                                                        | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `profile`                                                                        | [models.DeploymentComputePlanProfile](../models/deploymentcomputeplanprofile.md) | :heavy_check_mark:                                                               | N/A                                                                              |
| `recommended`                                                                    | *boolean*                                                                        | :heavy_check_mark:                                                               | N/A                                                                              |