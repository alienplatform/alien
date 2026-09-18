# DeploymentComputePlanProfile

## Example Usage

```typescript
import { DeploymentComputePlanProfile } from "@alienplatform/platform-api/models";

let value: DeploymentComputePlanProfile = {
  cpu: "<value>",
  memoryBytes: 794143,
  ephemeralStorageBytes: 818255,
};
```

## Fields

| Field                                                          | Type                                                           | Required                                                       | Description                                                    |
| -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- |
| `cpu`                                                          | *string*                                                       | :heavy_check_mark:                                             | N/A                                                            |
| `memoryBytes`                                                  | *number*                                                       | :heavy_check_mark:                                             | N/A                                                            |
| `ephemeralStorageBytes`                                        | *number*                                                       | :heavy_check_mark:                                             | N/A                                                            |
| `architecture`                                                 | [models.ProfileArchitecture](../models/profilearchitecture.md) | :heavy_minus_sign:                                             | N/A                                                            |
| `gpu`                                                          | [models.ProfileGpu](../models/profilegpu.md)                   | :heavy_minus_sign:                                             | N/A                                                            |