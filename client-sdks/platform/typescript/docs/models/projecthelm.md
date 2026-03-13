# ProjectHelm

Helm chart package configuration. If null, Helm packages will not be generated.

## Example Usage

```typescript
import { ProjectHelm } from "@aliendotdev/platform-api/models";

let value: ProjectHelm = {
  chartName: "<value>",
  description: "mid brr qua once yet fully",
  enabled: false,
};
```

## Fields

| Field                                            | Type                                             | Required                                         | Description                                      |
| ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ |
| `chartName`                                      | *string*                                         | :heavy_check_mark:                               | Chart name (e.g., "acme-operator")               |
| `description`                                    | *string*                                         | :heavy_check_mark:                               | Human-friendly description of the chart          |
| `enabled`                                        | *boolean*                                        | :heavy_check_mark:                               | Whether Helm chart package generation is enabled |