# DeveloperInputsByPlatform

## Example Usage

```typescript
import { DeveloperInputsByPlatform } from "@alienplatform/platform-api/models";

let value: DeveloperInputsByPlatform = {
  id: "<id>",
  label: "<value>",
  required: false,
};
```

## Fields

| Field                                                               | Type                                                                | Required                                                            | Description                                                         |
| ------------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------- |
| `id`                                                                | *string*                                                            | :heavy_check_mark:                                                  | Stable input ID used by CLI/API calls.                              |
| `label`                                                             | *string*                                                            | :heavy_check_mark:                                                  | Human-facing field label.                                           |
| `required`                                                          | *boolean*                                                           | :heavy_check_mark:                                                  | Whether a resolved value is required before deployment can proceed. |
| `generate`                                                          | *models.APIKeyDeploymentSetupConfigGenerateUnion*                   | :heavy_minus_sign:                                                  | N/A                                                                 |