# CreateProjectFromTemplateDeploymentPageBackground

Customization settings for the deployment page background animation.

## Example Usage

```typescript
import { CreateProjectFromTemplateDeploymentPageBackground } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectFromTemplateDeploymentPageBackground = {
  type: "gradient-mesh",
  mode: "dark",
  colorScheme: "blue",
};
```

## Fields

| Field                                                                                                                                                | Type                                                                                                                                                 | Required                                                                                                                                             | Description                                                                                                                                          | Example                                                                                                                                              |
| ---------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| `type`                                                                                                                                               | [operations.CreateProjectFromTemplateDeploymentPageBackgroundType](../../models/operations/createprojectfromtemplatedeploymentpagebackgroundtype.md) | :heavy_check_mark:                                                                                                                                   | Type of animated background to display on the deployment page.                                                                                       | gradient-mesh                                                                                                                                        |
| `mode`                                                                                                                                               | [operations.CreateProjectFromTemplateMode](../../models/operations/createprojectfromtemplatemode.md)                                                 | :heavy_check_mark:                                                                                                                                   | Color mode for the background animation.                                                                                                             | dark                                                                                                                                                 |
| `colorScheme`                                                                                                                                        | [operations.CreateProjectFromTemplateColorScheme](../../models/operations/createprojectfromtemplatecolorscheme.md)                                   | :heavy_check_mark:                                                                                                                                   | Color scheme for the background animation.                                                                                                           | blue                                                                                                                                                 |