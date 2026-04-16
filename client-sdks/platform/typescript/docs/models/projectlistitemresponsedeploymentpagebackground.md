# ProjectListItemResponseDeploymentPageBackground

Customization settings for the deployment page background animation.

## Example Usage

```typescript
import { ProjectListItemResponseDeploymentPageBackground } from "@alienplatform/platform-api/models";

let value: ProjectListItemResponseDeploymentPageBackground = {
  type: "gradient-mesh",
  mode: "dark",
  colorScheme: "blue",
};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    | Example                                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `type`                                                                                                                         | [models.ProjectListItemResponseDeploymentPageBackgroundType](../models/projectlistitemresponsedeploymentpagebackgroundtype.md) | :heavy_check_mark:                                                                                                             | Type of animated background to display on the deployment page.                                                                 | gradient-mesh                                                                                                                  |
| `mode`                                                                                                                         | [models.ProjectListItemResponseMode](../models/projectlistitemresponsemode.md)                                                 | :heavy_check_mark:                                                                                                             | Color mode for the background animation.                                                                                       | dark                                                                                                                           |
| `colorScheme`                                                                                                                  | [models.ProjectListItemResponseColorScheme](../models/projectlistitemresponsecolorscheme.md)                                   | :heavy_check_mark:                                                                                                             | Color scheme for the background animation.                                                                                     | blue                                                                                                                           |