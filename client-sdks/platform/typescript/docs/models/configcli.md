# ConfigCli

Configuration for the project CLI binary

## Example Usage

```typescript
import { ConfigCli } from "@aliendotdev/platform-api/models";

let value: ConfigCli = {
  displayName: "Triston_Hodkiewicz",
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