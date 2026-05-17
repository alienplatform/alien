# UpdateProjectDeploymentPortalAppearance

Customer-facing deployment portal appearance settings.

## Example Usage

```typescript
import { UpdateProjectDeploymentPortalAppearance } from "@alienplatform/platform-api/models/operations";

let value: UpdateProjectDeploymentPortalAppearance = {};
```

## Fields

| Field                                                                                       | Type                                                                                        | Required                                                                                    | Description                                                                                 |
| ------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| `avatarUrl`                                                                                 | *string*                                                                                    | :heavy_minus_sign:                                                                          | Optional project-specific avatar override for the deployment portal.                        |
| `preset`                                                                                    | [models.DeploymentPortalAppearancePreset](../../models/deploymentportalappearancepreset.md) | :heavy_minus_sign:                                                                          | Curated visual style for the deployment portal.                                             |
| `accentColor`                                                                               | [models.DeploymentPortalAccentColor](../../models/deploymentportalaccentcolor.md)           | :heavy_minus_sign:                                                                          | Accent color used for highlights and primary actions.                                       |
| `title`                                                                                     | *string*                                                                                    | :heavy_minus_sign:                                                                          | Optional portal title. Defaults to the project name.                                        |
| `subtitle`                                                                                  | *string*                                                                                    | :heavy_minus_sign:                                                                          | Optional customer-facing subtitle.                                                          |
| `supportUrl`                                                                                | *string*                                                                                    | :heavy_minus_sign:                                                                          | Optional support or contact URL.                                                            |
| `docsUrl`                                                                                   | *string*                                                                                    | :heavy_minus_sign:                                                                          | Optional documentation URL.                                                                 |
| `density`                                                                                   | [models.DeploymentPortalDensity](../../models/deploymentportaldensity.md)                   | :heavy_minus_sign:                                                                          | Layout density for portal content.                                                          |