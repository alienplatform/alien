# CreateProjectFromTemplateCliRequest

CLI package configuration. If null, CLI packages will not be generated.

## Example Usage

```typescript
import { CreateProjectFromTemplateCliRequest } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectFromTemplateCliRequest = {
  displayName: "Kadin.Weimann7",
  name: "<value>",
  enabled: true,
};
```

## Fields

| Field                                                         | Type                                                          | Required                                                      | Description                                                   |
| ------------------------------------------------------------- | ------------------------------------------------------------- | ------------------------------------------------------------- | ------------------------------------------------------------- |
| `displayName`                                                 | *string*                                                      | :heavy_check_mark:                                            | Human-friendly display name for help banners and about text   |
| `name`                                                        | *string*                                                      | :heavy_check_mark:                                            | Binary name displayed in help and usage (e.g., "acme-deploy") |
| `enabled`                                                     | *boolean*                                                     | :heavy_check_mark:                                            | Whether CLI package generation is enabled                     |