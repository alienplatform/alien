# UpdateDeploymentEnvironmentVariablesRequestVariable

## Example Usage

```typescript
import { UpdateDeploymentEnvironmentVariablesRequestVariable } from "@alienplatform/platform-api/models";

let value: UpdateDeploymentEnvironmentVariablesRequestVariable = {
  name: "<value>",
  type: "plain",
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `name`                                                                                                                 | *string*                                                                                                               | :heavy_check_mark:                                                                                                     | Variable name                                                                                                          |
| `value`                                                                                                                | *string*                                                                                                               | :heavy_minus_sign:                                                                                                     | New variable value. Omit to keep the stored value of an existing variable with the same name.                          |
| `type`                                                                                                                 | [models.UpdateDeploymentEnvironmentVariablesRequestType](../models/updatedeploymentenvironmentvariablesrequesttype.md) | :heavy_check_mark:                                                                                                     | Variable type                                                                                                          |
| `targetResources`                                                                                                      | *string*[]                                                                                                             | :heavy_minus_sign:                                                                                                     | Target resource patterns (null = all resources)                                                                        |