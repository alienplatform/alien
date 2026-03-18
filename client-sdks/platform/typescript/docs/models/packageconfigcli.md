# PackageConfigCli

## Example Usage

```typescript
import { PackageConfigCli } from "@alienplatform/platform-api/models";

let value: PackageConfigCli = {
  displayName: "Vada_Schamberger76",
  name: "<value>",
  type: "cli",
};
```

## Fields

| Field                                                       | Type                                                        | Required                                                    | Description                                                 |
| ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- |
| `displayName`                                               | *string*                                                    | :heavy_check_mark:                                          | Human-friendly display name for help banners and about text |
| `name`                                                      | *string*                                                    | :heavy_check_mark:                                          | Binary name displayed in help and usage (e.g., "my-cli")    |
| `type`                                                      | *"cli"*                                                     | :heavy_check_mark:                                          | N/A                                                         |