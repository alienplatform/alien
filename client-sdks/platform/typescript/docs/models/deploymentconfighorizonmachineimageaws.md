# DeploymentConfigHorizonMachineImageAws

AWS Horizon machine image catalog.

## Example Usage

```typescript
import { DeploymentConfigHorizonMachineImageAws } from "@alienplatform/platform-api/models";

let value: DeploymentConfigHorizonMachineImageAws = {
  amis: {},
};
```

## Fields

| Field                                     | Type                                      | Required                                  | Description                               |
| ----------------------------------------- | ----------------------------------------- | ----------------------------------------- | ----------------------------------------- |
| `amis`                                    | Record<string, Record<string, *string*>>  | :heavy_check_mark:                        | AMI IDs by architecture, then AWS region. |