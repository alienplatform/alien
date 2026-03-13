# CreateProjectCliResponse

CLI package configuration. If null, CLI packages will not be generated.

## Example Usage

```typescript
import { CreateProjectCliResponse } from "@aliendotdev/platform-api/models/operations";

let value: CreateProjectCliResponse = {
  displayName: "Madalyn.Zemlak42",
  name: "<value>",
  enabled: false,
};
```

## Fields

| Field                                                       | Type                                                        | Required                                                    | Description                                                 |
| ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- |
| `displayName`                                               | *string*                                                    | :heavy_check_mark:                                          | Human-friendly display name for help banners and about text |
| `name`                                                      | *string*                                                    | :heavy_check_mark:                                          | Binary name displayed in help and usage (e.g., "my-cli")    |
| `enabled`                                                   | *boolean*                                                   | :heavy_check_mark:                                          | Whether CLI package generation is enabled                   |