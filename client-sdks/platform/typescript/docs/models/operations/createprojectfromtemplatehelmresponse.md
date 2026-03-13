# CreateProjectFromTemplateHelmResponse

Helm chart package configuration. If null, Helm packages will not be generated.

## Example Usage

```typescript
import { CreateProjectFromTemplateHelmResponse } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectFromTemplateHelmResponse = {
  chartName: "<value>",
  description: "greatly super worth through",
  enabled: true,
};
```

## Fields

| Field                                            | Type                                             | Required                                         | Description                                      |
| ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ |
| `chartName`                                      | *string*                                         | :heavy_check_mark:                               | Chart name (e.g., "acme-operator")               |
| `description`                                    | *string*                                         | :heavy_check_mark:                               | Human-friendly description of the chart          |
| `enabled`                                        | *boolean*                                        | :heavy_check_mark:                               | Whether Helm chart package generation is enabled |