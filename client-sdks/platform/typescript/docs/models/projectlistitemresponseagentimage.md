# ProjectListItemResponseAgentImage

Agent image package configuration. Required when Helm is enabled. If null, agent image packages will not be generated.

## Example Usage

```typescript
import { ProjectListItemResponseAgentImage } from "@alienplatform/platform-api/models";

let value: ProjectListItemResponseAgentImage = {
  displayName: "Moses_Walker94",
  name: "<value>",
  enabled: false,
};
```

## Fields

| Field                                                     | Type                                                      | Required                                                  | Description                                               |
| --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- |
| `displayName`                                             | *string*                                                  | :heavy_check_mark:                                        | Human-friendly display name for logs and startup messages |
| `name`                                                    | *string*                                                  | :heavy_check_mark:                                        | Binary name (e.g., "acme-agent")                          |
| `enabled`                                                 | *boolean*                                                 | :heavy_check_mark:                                        | Whether agent image package generation is enabled         |