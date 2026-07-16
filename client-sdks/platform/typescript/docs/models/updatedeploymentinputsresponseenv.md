# UpdateDeploymentInputsResponseEnv

How a resolved stack input is injected into runtime environment variables.

## Example Usage

```typescript
import { UpdateDeploymentInputsResponseEnv } from "@alienplatform/platform-api/models";

let value: UpdateDeploymentInputsResponseEnv = {
  name: "<value>",
};
```

## Fields

| Field                                                                   | Type                                                                    | Required                                                                | Description                                                             |
| ----------------------------------------------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| `name`                                                                  | *string*                                                                | :heavy_check_mark:                                                      | Environment variable name.                                              |
| `targetResources`                                                       | *string*[]                                                              | :heavy_minus_sign:                                                      | Target resource IDs or patterns. None means every env-capable resource. |
| `type`                                                                  | *models.UpdateDeploymentInputsResponseTypeUnion*                        | :heavy_minus_sign:                                                      | N/A                                                                     |