# DeploymentConfigAwsClusters

AWS machine image catalog.

## Example Usage

```typescript
import { DeploymentConfigAwsClusters } from "@alienplatform/platform-api/models";

let value: DeploymentConfigAwsClusters = {
  amis: {},
};
```

## Fields

| Field                                     | Type                                      | Required                                  | Description                               |
| ----------------------------------------- | ----------------------------------------- | ----------------------------------------- | ----------------------------------------- |
| `amis`                                    | Record<string, Record<string, *string*>>  | :heavy_check_mark:                        | AMI IDs by architecture, then AWS region. |