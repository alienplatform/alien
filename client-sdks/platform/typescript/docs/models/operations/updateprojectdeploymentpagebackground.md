# UpdateProjectDeploymentPageBackground

Customization settings for the deployment page background animation.

## Example Usage

```typescript
import { UpdateProjectDeploymentPageBackground } from "@aliendotdev/platform-api/models/operations";

let value: UpdateProjectDeploymentPageBackground = {
  type: "gradient-mesh",
  mode: "dark",
  colorScheme: "blue",
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  | Example                                                                                                                      |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `type`                                                                                                                       | [operations.UpdateProjectDeploymentPageBackgroundType](../../models/operations/updateprojectdeploymentpagebackgroundtype.md) | :heavy_check_mark:                                                                                                           | Type of animated background to display on the deployment page.                                                               | gradient-mesh                                                                                                                |
| `mode`                                                                                                                       | [operations.UpdateProjectMode](../../models/operations/updateprojectmode.md)                                                 | :heavy_check_mark:                                                                                                           | Color mode for the background animation.                                                                                     | dark                                                                                                                         |
| `colorScheme`                                                                                                                | [operations.UpdateProjectColorScheme](../../models/operations/updateprojectcolorscheme.md)                                   | :heavy_check_mark:                                                                                                           | Color scheme for the background animation.                                                                                   | blue                                                                                                                         |