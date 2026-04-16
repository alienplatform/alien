# DeploymentInfoProject

## Example Usage

```typescript
import { DeploymentInfoProject } from "@alienplatform/platform-api/models";

let value: DeploymentInfoProject = {
  name: "<value>",
  workspace: "<value>",
  deploymentPageBackground: {
    type: "gradient-mesh",
    mode: "dark",
    colorScheme: "blue",
  },
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `name`                                                                   | *string*                                                                 | :heavy_check_mark:                                                       | N/A                                                                      |
| `workspace`                                                              | *string*                                                                 | :heavy_check_mark:                                                       | N/A                                                                      |
| `deploymentPageBackground`                                               | [models.DeploymentPageBackground](../models/deploymentpagebackground.md) | :heavy_minus_sign:                                                       | N/A                                                                      |