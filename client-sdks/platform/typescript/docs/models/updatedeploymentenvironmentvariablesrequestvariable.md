# UpdateDeploymentEnvironmentVariablesRequestVariable

## Example Usage

```typescript
import { UpdateDeploymentEnvironmentVariablesRequestVariable } from "@alienplatform/platform-api/models";

let value: UpdateDeploymentEnvironmentVariablesRequestVariable = {
  name: "<value>",
  value: "<value>",
  type: "plain",
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `name`                                                                                                                 | *string*                                                                                                               | :heavy_check_mark:                                                                                                     | Variable name                                                                                                          |
| `value`                                                                                                                | *string*                                                                                                               | :heavy_check_mark:                                                                                                     | Variable value                                                                                                         |
| `type`                                                                                                                 | [models.UpdateDeploymentEnvironmentVariablesRequestType](../models/updatedeploymentenvironmentvariablesrequesttype.md) | :heavy_check_mark:                                                                                                     | Variable type                                                                                                          |
| `targetResources`                                                                                                      | *string*[]                                                                                                             | :heavy_minus_sign:                                                                                                     | Target resource patterns (null = all resources)                                                                        |