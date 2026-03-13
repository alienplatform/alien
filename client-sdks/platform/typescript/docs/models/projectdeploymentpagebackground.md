# ProjectDeploymentPageBackground

Customization settings for the deployment page background animation.

## Example Usage

```typescript
import { ProjectDeploymentPageBackground } from "@aliendotdev/platform-api/models";

let value: ProjectDeploymentPageBackground = {
  type: "gradient-mesh",
  mode: "dark",
  colorScheme: "blue",
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    | Example                                                                                        |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `type`                                                                                         | [models.ProjectDeploymentPageBackgroundType](../models/projectdeploymentpagebackgroundtype.md) | :heavy_check_mark:                                                                             | Type of animated background to display on the deployment page.                                 | gradient-mesh                                                                                  |
| `mode`                                                                                         | [models.ProjectMode](../models/projectmode.md)                                                 | :heavy_check_mark:                                                                             | Color mode for the background animation.                                                       | dark                                                                                           |
| `colorScheme`                                                                                  | [models.ProjectColorScheme](../models/projectcolorscheme.md)                                   | :heavy_check_mark:                                                                             | Color scheme for the background animation.                                                     | blue                                                                                           |