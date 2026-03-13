# DeploymentPageBackground

## Example Usage

```typescript
import { DeploymentPageBackground } from "@aliendotdev/platform-api/models";

let value: DeploymentPageBackground = {
  type: "gradient-mesh",
  mode: "dark",
  colorScheme: "blue",
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    | Example                                                                                        |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `type`                                                                                         | [models.DeploymentPageBackgroundType](../models/deploymentpagebackgroundtype.md)               | :heavy_check_mark:                                                                             | Type of animated background to display on the deployment page.                                 | gradient-mesh                                                                                  |
| `mode`                                                                                         | [models.DeploymentPageBackgroundMode](../models/deploymentpagebackgroundmode.md)               | :heavy_check_mark:                                                                             | Color mode for the background animation.                                                       | dark                                                                                           |
| `colorScheme`                                                                                  | [models.DeploymentPageBackgroundColorScheme](../models/deploymentpagebackgroundcolorscheme.md) | :heavy_check_mark:                                                                             | Color scheme for the background animation.                                                     | blue                                                                                           |