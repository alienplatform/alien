# ProjectListItemResponseHelm

Helm chart package configuration. If null, Helm packages will not be generated.

## Example Usage

```typescript
import { ProjectListItemResponseHelm } from "@aliendotdev/platform-api/models";

let value: ProjectListItemResponseHelm = {
  chartName: "<value>",
  description:
    "optimal worthwhile phew whereas roughly noteworthy by duh formamide",
  enabled: false,
};
```

## Fields

| Field                                            | Type                                             | Required                                         | Description                                      |
| ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ |
| `chartName`                                      | *string*                                         | :heavy_check_mark:                               | Chart name (e.g., "acme-operator")               |
| `description`                                    | *string*                                         | :heavy_check_mark:                               | Human-friendly description of the chart          |
| `enabled`                                        | *boolean*                                        | :heavy_check_mark:                               | Whether Helm chart package generation is enabled |