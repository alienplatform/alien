# ConfigCli

Branding configuration for the deploy CLI binary

## Example Usage

```typescript
import { ConfigCli } from "@alienplatform/platform-api/models";

let value: ConfigCli = {
  displayName: "Triston_Hodkiewicz",
  name: "<value>",
  type: "cli",
};
```

## Fields

| Field                                                         | Type                                                          | Required                                                      | Description                                                   |
| ------------------------------------------------------------- | ------------------------------------------------------------- | ------------------------------------------------------------- | ------------------------------------------------------------- |
| `displayName`                                                 | *string*                                                      | :heavy_check_mark:                                            | Human-friendly display name for help banners and about text   |
| `name`                                                        | *string*                                                      | :heavy_check_mark:                                            | Binary name displayed in help and usage (e.g., "acme-deploy") |
| `type`                                                        | *"cli"*                                                       | :heavy_check_mark:                                            | N/A                                                           |