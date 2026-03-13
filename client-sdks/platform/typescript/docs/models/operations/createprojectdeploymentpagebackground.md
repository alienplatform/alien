# CreateProjectDeploymentPageBackground

Customization settings for the deployment page background animation.

## Example Usage

```typescript
import { CreateProjectDeploymentPageBackground } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectDeploymentPageBackground = {
  type: "gradient-mesh",
  mode: "dark",
  colorScheme: "blue",
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  | Example                                                                                                                      |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `type`                                                                                                                       | [operations.CreateProjectDeploymentPageBackgroundType](../../models/operations/createprojectdeploymentpagebackgroundtype.md) | :heavy_check_mark:                                                                                                           | Type of animated background to display on the deployment page.                                                               | gradient-mesh                                                                                                                |
| `mode`                                                                                                                       | [operations.CreateProjectMode](../../models/operations/createprojectmode.md)                                                 | :heavy_check_mark:                                                                                                           | Color mode for the background animation.                                                                                     | dark                                                                                                                         |
| `colorScheme`                                                                                                                | [operations.CreateProjectColorScheme](../../models/operations/createprojectcolorscheme.md)                                   | :heavy_check_mark:                                                                                                           | Color scheme for the background animation.                                                                                   | blue                                                                                                                         |